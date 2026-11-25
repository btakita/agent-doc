//! Passive GitHub-state observation for tracked work (`#operatorshouldnneed`).
//!
//! The operator used to ask for an `#audit-stale` pass: a worker checked every
//! pull request and issue the todo, backlog, review and icebox rows cite
//! against GitHub and reported which rows were done or moot. This module makes
//! the deterministic half of that audit background work:
//!
//! - **Source**: `.agent-doc/state/github-upstream-state.json`, one cached
//!   observation (`open`, `merged`, `closed`, or a failed lookup) per artifact,
//!   keyed by its decision-graph node id (`github:<owner>/<repo>/pull/<n>`).
//! - **Effect**: [`run_upstream_observation_on_admitted_cut`], the last pass of
//!   the deferred admission reconcile (`#q-e9483473162d`) that the controller's
//!   per-document maintenance worker runs after every admission. It refreshes
//!   the artifacts the open tracked items wait on
//!   ([`DecisionGraph::work_upstream_set`]) that are due, a bounded number per
//!   pass, and never writes the document. Terminal states are never refreshed;
//!   an open state is refreshed after [`OPEN_TTL_SECS`]; a failed lookup is
//!   retried after [`FAILED_RETRY_SECS`].
//! - **Computed**: preflight derives `stale_tracked_item` signals with
//!   `stale_kind` `upstream_merged` / `upstream_closed` from the cache alone
//!   ([`DecisionGraph::work_settled_by_upstream`]); admission never runs `gh`.
//!
//! [`DecisionGraph::work_upstream_set`]: agent_doc_answer_ledger::graph::DecisionGraph::work_upstream_set
//! [`DecisionGraph::work_settled_by_upstream`]: agent_doc_answer_ledger::graph::DecisionGraph::work_settled_by_upstream

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_doc_answer_ledger::graph::{GithubUpstream, GithubUpstreamKind, UpstreamState};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use wait_timeout::ChildExt;

/// Project-relative cache path.
pub const CACHE_RELATIVE: &str = ".agent-doc/state/github-upstream-state.json";
/// An `open` observation older than this is due again.
pub const OPEN_TTL_SECS: u64 = 30 * 60;
/// A failed lookup is retried after this long.
pub const FAILED_RETRY_SECS: u64 = 30 * 60;
/// Lookups one pass may run.
pub const LOOKUPS_PER_PASS: usize = 8;
/// Wall budget of one pass; a lookup is not started after it.
pub const PASS_BUDGET: Duration = Duration::from_secs(30);
/// One `gh` lookup's timeout.
pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
/// `off` disables the observation pass (the cache is still read).
pub const DISABLE_ENV: &str = "AGENT_DOC_UPSTREAM_OBSERVATION";
/// The `gh` program to run (default `gh`).
pub const GH_ENV: &str = "AGENT_DOC_GH";

/// One cached observation. `state: None` is a failed lookup at `observed_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedObservation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<UpstreamState>,
    pub observed_at: u64,
}

/// The project's observation cache.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamStateCache {
    #[serde(default)]
    pub upstreams: BTreeMap<String, CachedObservation>,
}

impl UpstreamStateCache {
    /// The last successful observation of `upstream`.
    pub fn state_of(&self, upstream: &GithubUpstream) -> Option<UpstreamState> {
        self.upstreams
            .get(&upstream.node_id())
            .and_then(|entry| entry.state)
    }

    /// Whether any observation is terminal: preflight skips the graph build
    /// when none is.
    pub fn has_terminal(&self) -> bool {
        self.upstreams
            .values()
            .any(|entry| entry.state.is_some_and(UpstreamState::is_terminal))
    }

    /// Whether `upstream` needs a lookup at `now`.
    pub fn is_due(&self, upstream: &GithubUpstream, now: u64) -> bool {
        let Some(entry) = self.upstreams.get(&upstream.node_id()) else {
            return true;
        };
        let age = now.saturating_sub(entry.observed_at);
        match entry.state {
            Some(state) if state.is_terminal() => false,
            Some(_) => age >= OPEN_TTL_SECS,
            None => age >= FAILED_RETRY_SECS,
        }
    }

    /// The due artifacts of `wanted`, at most `limit`: never-observed first,
    /// then the oldest observation.
    pub fn plan_refresh(
        &self,
        wanted: &BTreeSet<GithubUpstream>,
        now: u64,
        limit: usize,
    ) -> Vec<GithubUpstream> {
        let mut due: Vec<(u64, &GithubUpstream)> = wanted
            .iter()
            .filter(|upstream| self.is_due(upstream, now))
            .map(|upstream| {
                let seen = self
                    .upstreams
                    .get(&upstream.node_id())
                    .map_or(0, |entry| entry.observed_at.saturating_add(1));
                (seen, upstream)
            })
            .collect();
        due.sort();
        due.into_iter()
            .take(limit)
            .map(|(_, upstream)| upstream.clone())
            .collect()
    }

    fn record(&mut self, upstream: &GithubUpstream, state: Option<UpstreamState>, now: u64) {
        let key = upstream.node_id();
        // A failed lookup never erases a known state; it only defers the retry.
        let state = state.or_else(|| self.upstreams.get(&key).and_then(|entry| entry.state));
        self.upstreams.insert(
            key,
            CachedObservation {
                state,
                observed_at: now,
            },
        );
    }
}

pub fn cache_path(root: &Path) -> PathBuf {
    root.join(CACHE_RELATIVE)
}

/// The project's cache; missing or unreadable is empty.
pub fn load(root: &Path) -> UpstreamStateCache {
    std::fs::read_to_string(cache_path(root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The cache of the project that contains `file`.
pub fn load_for(file: &Path) -> UpstreamStateCache {
    agent_doc_project_root_io::project_root_containing(file)
        .map(|root| load(&root))
        .unwrap_or_default()
}

fn store(root: &Path, cache: &UpstreamStateCache) -> Result<()> {
    let path = cache_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(cache)? + "\n")?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// What one observation pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObservationReport {
    pub wanted: usize,
    pub observed: Vec<(GithubUpstream, UpstreamState)>,
    pub failed: Vec<GithubUpstream>,
    /// Due but left for a later pass (limit or wall budget).
    pub deferred: usize,
}

/// Refresh the due artifacts of `wanted` with `lookup`, at most `limit`, not
/// starting a lookup after `budget`. The cache is re-read before the write so
/// a concurrent pass for another document keeps its observations.
pub fn observe(
    root: &Path,
    wanted: &BTreeSet<GithubUpstream>,
    now: u64,
    limit: usize,
    budget: Duration,
    lookup: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
) -> Result<ObservationReport> {
    let planned = load(root).plan_refresh(wanted, now, usize::MAX);
    let mut report = ObservationReport {
        wanted: wanted.len(),
        ..ObservationReport::default()
    };
    let started = Instant::now();
    let mut results = Vec::new();
    for upstream in &planned {
        if results.len() >= limit || started.elapsed() >= budget {
            report.deferred += 1;
            continue;
        }
        let state = lookup(upstream);
        match state {
            Some(state) => report.observed.push((upstream.clone(), state)),
            None => report.failed.push(upstream.clone()),
        }
        results.push((upstream.clone(), state));
    }
    if !results.is_empty() {
        let mut cache = load(root);
        for (upstream, state) in &results {
            cache.record(upstream, *state, now);
        }
        store(root, &cache)?;
    }
    Ok(report)
}

/// `gh pr view` / `gh issue view` for one artifact. `None` when `gh` is
/// missing, unauthenticated, times out, or prints no state.
pub fn github_state(upstream: &GithubUpstream) -> Option<UpstreamState> {
    let program = std::env::var_os(GH_ENV).unwrap_or_else(|| "gh".into());
    github_state_with(&program, upstream)
}

/// [`github_state`] with an explicit `gh` program.
pub fn github_state_with(
    program: &std::ffi::OsStr,
    upstream: &GithubUpstream,
) -> Option<UpstreamState> {
    let kind = match upstream.kind {
        GithubUpstreamKind::Issue => "issue",
        GithubUpstreamKind::PullRequest => "pr",
    };
    let repo = format!("{}/{}", upstream.owner, upstream.repo);
    let mut attempt = 0;
    let mut child = loop {
        let spawned = Command::new(program)
            .args([
                kind,
                "view",
                &upstream.number.to_string(),
                "--repo",
                &repo,
                "--json",
                "state",
                "--jq",
                ".state",
            ])
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        // ETXTBSY: the executable is being replaced (an upgrade) or was just
        // written; it clears within milliseconds.
        match spawned {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 5 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => break other.ok()?,
        }
    };
    let status = match child.wait_timeout(LOOKUP_TIMEOUT) {
        Ok(Some(status)) => status,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    if !status.success() {
        return None;
    }
    let mut stdout = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut stdout).ok()?;
    UpstreamState::parse_gh_state(&stdout)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

/// The observation pass over `content` with `lookup`. Never writes the
/// document; returns the report (`None` when nothing was wanted or the
/// project root is unknown).
pub fn observe_document(
    file: &Path,
    content: &str,
    now: u64,
    lookup: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
) -> Result<Option<ObservationReport>> {
    let Some(root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(None);
    };
    let wanted = crate::answer_ledger::work_upstream_set(file, content);
    if wanted.is_empty() {
        return Ok(None);
    }
    observe(&root, &wanted, now, LOOKUPS_PER_PASS, PASS_BUDGET, lookup).map(Some)
}

/// The deferred admission reconcile's observation pass (`#operatorshouldnneed`).
/// Read-only on the document, so it always returns `Ok(None)` and runs last:
/// its lookups never delay a pass that writes. Logged as
/// `upstream_observation`.
pub fn run_upstream_observation_on_admitted_cut(
    file: &Path,
    content: &str,
    _fence: &dyn crate::PendingMaintenanceEffectFence,
) -> Result<Option<String>> {
    if disabled() {
        return Ok(None);
    }
    let Some(report) = observe_document(file, content, now_secs(), &github_state)? else {
        return Ok(None);
    };
    if report.observed.is_empty() && report.failed.is_empty() {
        return Ok(None);
    }
    let terminal = report
        .observed
        .iter()
        .filter(|(_, state)| state.is_terminal())
        .count();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "upstream_observation file={} wanted={} observed={} terminal={terminal} failed={} deferred={} (#operatorshouldnneed)",
            file.display(),
            report.wanted,
            report.observed.len(),
            report.failed.len(),
            report.deferred,
        ),
    );
    Ok(None)
}

/// One watched artifact and its cached observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchedUpstream {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<UpstreamState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    pub due: bool,
}

/// `agent-doc decision upstreams`: the watch set, its cached states, and the
/// items those states settle.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpstreamReport {
    pub watched: Vec<WatchedUpstream>,
    pub settled: Vec<crate::TodoSignal>,
}

/// The watch set of `content` with the cached states at `now`, and the
/// `stale_tracked_item` signals preflight would raise. Read-only.
pub fn report(file: &Path, content: &str, now: u64) -> UpstreamReport {
    let cache = load_for(file);
    let watched = crate::answer_ledger::work_upstream_set(file, content)
        .into_iter()
        .map(|upstream| {
            let entry = cache.upstreams.get(&upstream.node_id());
            WatchedUpstream {
                url: upstream.url(),
                state: entry.and_then(|entry| entry.state),
                observed_at: entry.map(|entry| entry.observed_at),
                due: cache.is_due(&upstream, now),
            }
        })
        .collect();
    UpstreamReport {
        watched,
        settled: crate::answer_ledger::upstream_settled_signals(file, content),
    }
}

/// The current time in seconds, as the observation pass records it.
pub fn now() -> u64 {
    now_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn pull(number: u64) -> GithubUpstream {
        GithubUpstream::pull_request("o", "r", number)
    }

    fn wanted(numbers: &[u64]) -> BTreeSet<GithubUpstream> {
        numbers.iter().copied().map(pull).collect()
    }

    #[test]
    fn terminal_states_are_never_due_and_open_ones_expire() {
        let mut cache = UpstreamStateCache::default();
        cache.record(&pull(1), Some(UpstreamState::Merged), 100);
        cache.record(&pull(2), Some(UpstreamState::Open), 100);
        cache.record(&pull(3), None, 100);
        let later = 100 + OPEN_TTL_SECS.max(FAILED_RETRY_SECS);
        assert!(!cache.is_due(&pull(1), later * 10));
        assert!(!cache.is_due(&pull(2), 101));
        assert!(cache.is_due(&pull(2), later));
        assert!(!cache.is_due(&pull(3), 101));
        assert!(cache.is_due(&pull(3), later));
        assert!(cache.is_due(&pull(4), 0));
        assert!(cache.has_terminal());
    }

    #[test]
    fn a_failed_lookup_keeps_the_known_state() {
        let mut cache = UpstreamStateCache::default();
        cache.record(&pull(1), Some(UpstreamState::Open), 100);
        cache.record(&pull(1), None, 5000);
        assert_eq!(cache.state_of(&pull(1)), Some(UpstreamState::Open));
        assert!(!cache.is_due(&pull(1), 5001));
    }

    #[test]
    fn refresh_plans_unobserved_first_then_oldest() {
        let mut cache = UpstreamStateCache::default();
        cache.record(&pull(1), Some(UpstreamState::Open), 10);
        cache.record(&pull(2), Some(UpstreamState::Open), 5);
        cache.record(&pull(3), Some(UpstreamState::Merged), 1);
        let now = 10 + OPEN_TTL_SECS;
        let plan = cache.plan_refresh(&wanted(&[1, 2, 3, 4]), now, 3);
        assert_eq!(plan, vec![pull(4), pull(2), pull(1)]);
        assert_eq!(
            cache.plan_refresh(&wanted(&[1, 2, 3, 4]), now, 1),
            vec![pull(4)]
        );
    }

    #[test]
    fn observe_is_bounded_records_results_and_skips_fresh_entries() {
        let dir = tempfile::tempdir().unwrap();
        let calls = RefCell::new(Vec::new());
        let lookup = |upstream: &GithubUpstream| {
            calls.borrow_mut().push(upstream.number);
            match upstream.number {
                1 => Some(UpstreamState::Merged),
                2 => Some(UpstreamState::Open),
                _ => None,
            }
        };
        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            100,
            2,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(report.wanted, 3);
        assert_eq!(report.observed.len(), 2);
        assert_eq!(report.deferred, 1);
        let cache = load(dir.path());
        assert_eq!(cache.state_of(&pull(1)), Some(UpstreamState::Merged));
        assert_eq!(cache.state_of(&pull(2)), Some(UpstreamState::Open));

        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            101,
            8,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(
            report.failed,
            vec![pull(3)],
            "only the unobserved one is due"
        );
        assert_eq!(*calls.borrow(), vec![1, 2, 3]);
        assert!(load(dir.path()).upstreams.contains_key(&pull(3).node_id()));

        // Nothing due: no lookup and no write.
        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            102,
            8,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert!(report.observed.is_empty() && report.failed.is_empty());
        assert_eq!(calls.borrow().len(), 3);
    }

    #[test]
    fn a_zero_budget_defers_every_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let lookup = |_: &GithubUpstream| -> Option<UpstreamState> { panic!("no lookup") };
        let report = observe(dir.path(), &wanted(&[1, 2]), 0, 8, Duration::ZERO, &lookup).unwrap();
        assert_eq!(report.deferred, 2);
        assert!(!cache_path(dir.path()).exists());
    }

    #[test]
    fn a_corrupt_cache_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(cache_path(dir.path()).parent().unwrap()).unwrap();
        std::fs::write(cache_path(dir.path()), "not json").unwrap();
        assert_eq!(load(dir.path()), UpstreamStateCache::default());
    }

    #[cfg(unix)]
    #[test]
    fn github_state_reads_the_gh_program_output() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("gh");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1 $3\" in\n  'pr 7') echo MERGED ;;\n  'issue 8') echo CLOSED ;;\n  *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let gh = fake.as_os_str();
        assert_eq!(github_state_with(gh, &pull(7)), Some(UpstreamState::Merged));
        assert_eq!(
            github_state_with(gh, &GithubUpstream::issue("o", "r", 8)),
            Some(UpstreamState::Closed)
        );
        assert_eq!(github_state_with(gh, &pull(9)), None);
        assert_eq!(
            github_state_with(dir.path().join("missing").as_os_str(), &pull(7)),
            None
        );
    }
}
