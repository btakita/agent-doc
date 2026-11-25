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
//!   ([`DecisionGraph::work_upstream_set`]) that are due, with at most
//!   [`CALLS_PER_PASS`] batched `gh api graphql` calls per pass of up to
//!   [`ARTIFACTS_PER_CALL`] artifacts each, and never writes the document. Terminal states are never refreshed;
//!   an open state is refreshed after [`OPEN_TTL_SECS`]; a failed lookup is
//!   retried after [`FAILED_LOOKUP_TTL_SECS`].
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
pub const FAILED_LOOKUP_TTL_SECS: u64 = 30 * 60;
/// `gh` calls one pass may run. Each call is one GraphQL query that looks up
/// up to [`ARTIFACTS_PER_CALL`] artifacts, across repositories.
pub const CALLS_PER_PASS: usize = 8;
/// Artifacts one batched GraphQL call looks up.
pub const ARTIFACTS_PER_CALL: usize = 50;
/// Wall budget of one pass; a call is not started after it.
pub const PASS_BUDGET: Duration = Duration::from_secs(30);
/// One `gh` call's timeout.
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
            None => age >= FAILED_LOOKUP_TTL_SECS,
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
    /// Due but left for a later pass (call cap or wall budget).
    pub deferred: usize,
    /// Batched lookup calls the pass ran.
    pub calls: usize,
}

/// A batched lookup: one state per artifact of the batch, in order. `None` is
/// a failed lookup of that artifact only; a short result fails the rest.
pub type BatchLookup<'a> = dyn Fn(&[GithubUpstream]) -> Vec<Option<UpstreamState>> + 'a;

/// Refresh the due artifacts of `wanted` with `lookup`: at most `max_calls`
/// calls of at most `batch_size` artifacts each, not starting a call after
/// `budget`. The cache is re-read before the write so a concurrent pass for
/// another document keeps its observations.
pub fn observe(
    root: &Path,
    wanted: &BTreeSet<GithubUpstream>,
    now: u64,
    max_calls: usize,
    batch_size: usize,
    budget: Duration,
    lookup: &BatchLookup<'_>,
) -> Result<ObservationReport> {
    let planned = load(root).plan_refresh(wanted, now, usize::MAX);
    let mut report = ObservationReport {
        wanted: wanted.len(),
        ..ObservationReport::default()
    };
    let started = Instant::now();
    let mut results = Vec::new();
    for batch in planned.chunks(batch_size.max(1)) {
        if report.calls >= max_calls || started.elapsed() >= budget {
            report.deferred += batch.len();
            continue;
        }
        report.calls += 1;
        let states = lookup(batch);
        for (index, upstream) in batch.iter().enumerate() {
            let state = states.get(index).copied().flatten();
            match state {
                Some(state) => report.observed.push((upstream.clone(), state)),
                None => report.failed.push(upstream.clone()),
            }
            results.push((upstream.clone(), state));
        }
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

/// The GraphQL query that looks up every artifact of `batch` in one call:
/// one aliased `repository` per repository (`r<i>`) and one aliased field
/// per artifact (`a<j>`, `j` its index in `batch`). A pull request is read
/// with `pullRequest(number:)`; an issue with `issueOrPullRequest(number:)`,
/// because an `/issues/<n>` URL also resolves to a pull request.
pub fn graphql_query(batch: &[GithubUpstream]) -> String {
    let mut repos: Vec<((&str, &str), Vec<usize>)> = Vec::new();
    for (index, upstream) in batch.iter().enumerate() {
        let key = (upstream.owner.as_str(), upstream.repo.as_str());
        match repos.iter_mut().find(|(repo, _)| *repo == key) {
            Some((_, indexes)) => indexes.push(index),
            None => repos.push((key, vec![index])),
        }
    }
    let mut query = String::from("query{");
    for (repo_index, ((owner, repo), indexes)) in repos.iter().enumerate() {
        query.push_str(&format!(
            "r{repo_index}:repository(owner:{},name:{}){{",
            graphql_string(owner),
            graphql_string(repo)
        ));
        for &index in indexes {
            let upstream = &batch[index];
            match upstream.kind {
                GithubUpstreamKind::PullRequest => query.push_str(&format!(
                    "a{index}:pullRequest(number:{}){{state}}",
                    upstream.number
                )),
                GithubUpstreamKind::Issue => query.push_str(&format!(
                    "a{index}:issueOrPullRequest(number:{}){{...on Issue{{state}}...on PullRequest{{state}}}}",
                    upstream.number
                )),
            }
        }
        query.push('}');
    }
    query.push('}');
    query
}

fn graphql_string(text: &str) -> String {
    // A JSON string literal is a valid GraphQL string literal.
    serde_json::Value::String(text.to_string()).to_string()
}

/// The states [`graphql_query`] returned for `batch`, in order. GitHub
/// answers a partial query with `data` plus `errors`: a missing or forbidden
/// repository or artifact is `null` under its alias and fails only that
/// artifact.
pub fn parse_graphql_response(batch: &[GithubUpstream], text: &str) -> Vec<Option<UpstreamState>> {
    let Ok(body) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![None; batch.len()];
    };
    let Some(data) = body.get("data").and_then(serde_json::Value::as_object) else {
        return vec![None; batch.len()];
    };
    // Each artifact alias is unique across the query, so the repository it
    // sits under does not matter.
    let mut states: BTreeMap<String, UpstreamState> = BTreeMap::new();
    for repo in data.values().filter_map(serde_json::Value::as_object) {
        for (alias, artifact) in repo {
            if let Some(state) = artifact
                .get("state")
                .and_then(serde_json::Value::as_str)
                .and_then(UpstreamState::parse_gh_state)
            {
                states.insert(alias.clone(), state);
            }
        }
    }
    (0..batch.len())
        .map(|index| states.get(&format!("a{index}")).copied())
        .collect()
}

/// One batched `gh api graphql` call for `batch`. Every artifact fails when
/// `gh` is missing, unauthenticated, times out, or prints no `data`.
pub fn github_states(batch: &[GithubUpstream]) -> Vec<Option<UpstreamState>> {
    let program = std::env::var_os(GH_ENV).unwrap_or_else(|| "gh".into());
    github_states_with(&program, batch)
}

/// [`github_states`] for one artifact.
pub fn github_state(upstream: &GithubUpstream) -> Option<UpstreamState> {
    github_states(std::slice::from_ref(upstream))
        .into_iter()
        .next()
        .flatten()
}

/// [`github_states`] with an explicit `gh` program.
pub fn github_states_with(
    program: &std::ffi::OsStr,
    batch: &[GithubUpstream],
) -> Vec<Option<UpstreamState>> {
    if batch.is_empty() {
        return Vec::new();
    }
    let query = format!("query={}", graphql_query(batch));
    match run_gh(program, &["api", "graphql", "-f", &query]) {
        Some(stdout) => parse_graphql_response(batch, &stdout),
        None => vec![None; batch.len()],
    }
}

/// Run `gh` with `args` and return its stdout, also on a nonzero exit:
/// `gh api graphql` exits 1 on a partial response yet prints its `data`.
/// `None` when it cannot start or times out.
fn run_gh(program: &std::ffi::OsStr, args: &[&str]) -> Option<String> {
    let mut attempt = 0;
    let mut child = loop {
        let spawned = Command::new(program)
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        // ETXTBSY: the executable is being replaced (an upgrade) or was just
        // written while another thread forked; it clears within milliseconds.
        match spawned {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 50 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => break other.ok()?,
        }
    };
    // Drain stdout on a thread: a large response must not fill the pipe and
    // stall the child past its timeout.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut text).ok()?;
        Some(text)
    });
    match child.wait_timeout(LOOKUP_TIMEOUT) {
        Ok(Some(_)) => {}
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return None;
        }
    }
    reader.join().ok().flatten()
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
    lookup: &BatchLookup<'_>,
) -> Result<Option<ObservationReport>> {
    let Some(root) = agent_doc_project_root_io::project_root_containing(file) else {
        return Ok(None);
    };
    let wanted = crate::answer_ledger::work_upstream_set(file, content);
    if wanted.is_empty() {
        return Ok(None);
    }
    observe(
        &root,
        &wanted,
        now,
        CALLS_PER_PASS,
        ARTIFACTS_PER_CALL,
        PASS_BUDGET,
        lookup,
    )
    .map(Some)
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
    let Some(report) = observe_document(file, content, now_secs(), &github_states)? else {
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
            "upstream_observation file={} wanted={} observed={} terminal={terminal} failed={} deferred={} calls={} (#operatorshouldnneed)",
            file.display(),
            report.wanted,
            report.observed.len(),
            report.failed.len(),
            report.deferred,
            report.calls,
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

impl UpstreamStateCache {
    /// The terminal observations, in the form `queue tree-status` reads
    /// (`#operatorshouldnneed`): a live URL heading whose artifact is merged or closed is
    /// listed under Settled upstream. Open, failed and unparseable entries
    /// are left out. Reads the cache only; never runs `gh`.
    pub fn heading_states(&self) -> agent_doc_queue::tree_status::UpstreamStates {
        use agent_doc_queue::tree_status::SettledUpstreamState;
        let mut states = agent_doc_queue::tree_status::UpstreamStates::default();
        for (node_id, entry) in &self.upstreams {
            let state = match entry.state {
                Some(UpstreamState::Merged) => SettledUpstreamState::Merged,
                Some(UpstreamState::Closed) => SettledUpstreamState::Closed,
                Some(UpstreamState::Open) | None => continue,
            };
            if let Some(upstream) = GithubUpstream::from_node_id(node_id) {
                states.insert(&upstream.url(), state, entry.observed_at);
            }
        }
        states
    }
}

/// [`UpstreamStateCache::heading_states`] of the project containing `file`.
pub fn heading_states_for(file: &Path) -> agent_doc_queue::tree_status::UpstreamStates {
    load_for(file).heading_states()
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
        let later = 100 + OPEN_TTL_SECS.max(FAILED_LOOKUP_TTL_SECS);
        assert!(!cache.is_due(&pull(1), later * 10));
        assert!(!cache.is_due(&pull(2), 101));
        assert!(cache.is_due(&pull(2), later));
        assert!(!cache.is_due(&pull(3), 101));
        assert!(cache.is_due(&pull(3), later));
        assert!(cache.is_due(&pull(4), 0));
        assert!(cache.has_terminal());
    }

    /// `#operatorshouldnneed`: `queue tree-status` reads the terminal observations only,
    /// from the project cache, keyed by the artifact URL.
    #[test]
    fn settled_upstream_heading_states_are_the_terminal_observations_of_the_project_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "<!-- agent:queue id=\"main\" -->\n",
            "- ### https://github.com/o/r/pull/1\n",
            "  - %% merged %%\n",
            "- ### https://github.com/o/r/issues/2\n",
            "  - %% closed %%\n",
            "- ### https://github.com/o/r/pull/3\n",
            "  - %% open %%\n",
            "- ### https://github.com/o/r/pull/4\n",
            "  - %% lookup failed %%\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let mut cache = UpstreamStateCache::default();
        cache.record(&pull(1), Some(UpstreamState::Merged), 1_795_521_600);
        cache.record(
            &GithubUpstream::issue("o", "r", 2),
            Some(UpstreamState::Closed),
            1_795_521_600,
        );
        cache.record(&pull(3), Some(UpstreamState::Open), 1_795_521_600);
        cache.record(&pull(4), None, 1_795_521_600);
        store(dir.path(), &cache).unwrap();

        let states = heading_states_for(&doc);
        assert_eq!(states.len(), 2, "{states:?}");
        let status =
            agent_doc_queue::tree_status::tree_status_with_upstreams(content, 1, &states).unwrap();
        let settled = status
            .settled_upstream
            .iter()
            .map(|row| (row.url.as_str(), row.upstream_state.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            settled,
            vec![
                ("https://github.com/o/r/pull/1", "merged"),
                ("https://github.com/o/r/issues/2", "closed"),
            ]
        );
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

    /// A batch lookup from a per-artifact answer, recording each call's
    /// artifact numbers.
    fn batch_lookup<'a>(
        calls: &'a RefCell<Vec<Vec<u64>>>,
        answer: impl Fn(u64) -> Option<UpstreamState> + 'a,
    ) -> impl Fn(&[GithubUpstream]) -> Vec<Option<UpstreamState>> + 'a {
        move |batch: &[GithubUpstream]| {
            calls
                .borrow_mut()
                .push(batch.iter().map(|upstream| upstream.number).collect());
            batch
                .iter()
                .map(|upstream| answer(upstream.number))
                .collect()
        }
    }

    #[test]
    fn observe_is_bounded_records_results_and_skips_fresh_entries() {
        let dir = tempfile::tempdir().unwrap();
        let calls = RefCell::new(Vec::new());
        let lookup = batch_lookup(&calls, |number| match number {
            1 => Some(UpstreamState::Merged),
            2 => Some(UpstreamState::Open),
            _ => None,
        });
        // One call of two artifacts; the third waits for the next pass.
        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            100,
            1,
            2,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(report.wanted, 3);
        assert_eq!(report.calls, 1);
        assert_eq!(report.observed.len(), 2);
        assert_eq!(report.deferred, 1);
        let cache = load(dir.path());
        assert_eq!(cache.state_of(&pull(1)), Some(UpstreamState::Merged));
        assert_eq!(cache.state_of(&pull(2)), Some(UpstreamState::Open));

        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            101,
            CALLS_PER_PASS,
            ARTIFACTS_PER_CALL,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(
            report.failed,
            vec![pull(3)],
            "only the unobserved one is due"
        );
        assert_eq!(*calls.borrow(), vec![vec![1, 2], vec![3]]);
        assert!(load(dir.path()).upstreams.contains_key(&pull(3).node_id()));

        // Nothing due: no call and no write.
        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            102,
            CALLS_PER_PASS,
            ARTIFACTS_PER_CALL,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert!(report.observed.is_empty() && report.failed.is_empty());
        assert_eq!(report.calls, 0);
        assert_eq!(calls.borrow().len(), 2);
    }

    #[test]
    fn a_large_watch_set_takes_one_call_per_batch_not_per_artifact() {
        // sdk-dev.md's 20 watched pull requests took 3 passes of per-artifact
        // lookups (8/8/4); batched, they take one call.
        let dir = tempfile::tempdir().unwrap();
        let numbers: Vec<u64> = (1..=20).collect();
        let calls = RefCell::new(Vec::new());
        let lookup = batch_lookup(&calls, |_| Some(UpstreamState::Merged));
        let report = observe(
            dir.path(),
            &wanted(&numbers),
            100,
            CALLS_PER_PASS,
            ARTIFACTS_PER_CALL,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(report.calls, 1);
        assert_eq!(report.observed.len(), 20);
        assert_eq!(report.deferred, 0);

        // The cap counts calls: 2 calls of 8 cover 16, the last 4 wait.
        let dir = tempfile::tempdir().unwrap();
        calls.borrow_mut().clear();
        let report = observe(
            dir.path(),
            &wanted(&numbers),
            100,
            2,
            8,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(report.calls, 2);
        assert_eq!(report.observed.len(), 16);
        assert_eq!(report.deferred, 4);
        assert_eq!(
            calls.borrow().iter().map(Vec::len).collect::<Vec<_>>(),
            vec![8, 8]
        );
    }

    #[test]
    fn one_failed_artifact_does_not_poison_its_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = UpstreamStateCache::default();
        cache.record(&pull(2), Some(UpstreamState::Open), 1);
        store(dir.path(), &cache).unwrap();
        let calls = RefCell::new(Vec::new());
        // A short result fails only the artifacts it does not cover.
        let lookup = |batch: &[GithubUpstream]| {
            calls.borrow_mut().push(batch.len());
            vec![Some(UpstreamState::Merged), None]
        };
        let report = observe(
            dir.path(),
            &wanted(&[1, 2, 3]),
            1 + OPEN_TTL_SECS,
            CALLS_PER_PASS,
            ARTIFACTS_PER_CALL,
            PASS_BUDGET,
            &lookup,
        )
        .unwrap();
        assert_eq!(*calls.borrow(), vec![3]);
        // Planned never-observed first: 1, 3, then the stale open 2.
        assert_eq!(report.observed, vec![(pull(1), UpstreamState::Merged)]);
        assert_eq!(report.failed, vec![pull(3), pull(2)]);
        let cache = load(dir.path());
        assert_eq!(cache.state_of(&pull(1)), Some(UpstreamState::Merged));
        assert_eq!(cache.state_of(&pull(2)), Some(UpstreamState::Open));
        assert_eq!(cache.state_of(&pull(3)), None);
    }

    #[test]
    fn a_zero_budget_defers_every_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let lookup = |_: &[GithubUpstream]| -> Vec<Option<UpstreamState>> { panic!("no lookup") };
        let report = observe(
            dir.path(),
            &wanted(&[1, 2]),
            0,
            CALLS_PER_PASS,
            ARTIFACTS_PER_CALL,
            Duration::ZERO,
            &lookup,
        )
        .unwrap();
        assert_eq!(report.deferred, 2);
        assert_eq!(report.calls, 0);
        assert!(!cache_path(dir.path()).exists());
    }

    #[test]
    fn the_query_groups_artifacts_by_repository_with_unique_aliases() {
        let batch = vec![
            pull(7),
            GithubUpstream::issue("x", "y", 8),
            GithubUpstream::issue("o", "r", 9),
        ];
        assert_eq!(
            graphql_query(&batch),
            concat!(
                "query{",
                "r0:repository(owner:\"o\",name:\"r\"){",
                "a0:pullRequest(number:7){state}",
                "a2:issueOrPullRequest(number:9){...on Issue{state}...on PullRequest{state}}",
                "}",
                "r1:repository(owner:\"x\",name:\"y\"){",
                "a1:issueOrPullRequest(number:8){...on Issue{state}...on PullRequest{state}}",
                "}}",
            )
        );
        // Names are string literals, never raw query text.
        let hostile = GithubUpstream::pull_request("o\"){x}", "r", 1);
        assert!(graphql_query(&[hostile]).contains(r#"owner:"o\"){x}""#));
    }

    #[test]
    fn a_partial_graphql_response_fails_only_the_missing_artifacts() {
        let batch = vec![
            pull(1),
            GithubUpstream::issue("o", "r", 2),
            pull(999),
            GithubUpstream::issue("gone", "repo", 3),
        ];
        // `gh api graphql` output for a partial answer (exit status 1).
        let response = r#"{"data":{"r0":{"a0":{"state":"MERGED"},"a1":{"state":"CLOSED"},"a2":null},"r1":null},"errors":[{"type":"NOT_FOUND","path":["r1"]},{"type":"NOT_FOUND","path":["r0","a2"]}]}"#;
        assert_eq!(
            parse_graphql_response(&batch, response),
            vec![
                Some(UpstreamState::Merged),
                Some(UpstreamState::Closed),
                None,
                None,
            ]
        );
        assert_eq!(parse_graphql_response(&batch, "not json"), vec![None; 4]);
        assert_eq!(
            parse_graphql_response(&batch, r#"{"errors":[{"message":"Bad credentials"}]}"#),
            vec![None; 4]
        );
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
    fn github_states_runs_one_graphql_call_and_reads_a_partial_answer() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let fake = dir.path().join("gh");
        // Partial answer: `gh` prints the data and exits 1.
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '{}'\nexit 1\n",
                args.display(),
                r#"{"data":{"r0":{"a0":{"state":"MERGED"},"a1":null},"r1":{"a2":{"state":"OPEN"}}},"errors":[{"type":"NOT_FOUND","path":["r0","a1"]}]}"#,
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let batch = vec![pull(7), pull(9), GithubUpstream::issue("x", "y", 8)];
        assert_eq!(
            github_states_with(fake.as_os_str(), &batch),
            vec![Some(UpstreamState::Merged), None, Some(UpstreamState::Open)]
        );
        let recorded = std::fs::read_to_string(&args).unwrap();
        assert_eq!(
            recorded,
            format!("api\ngraphql\n-f\nquery={}\n", graphql_query(&batch))
        );
        assert_eq!(
            github_states_with(dir.path().join("missing").as_os_str(), &batch),
            vec![None; 3]
        );
        assert!(github_states_with(fake.as_os_str(), &[]).is_empty());
    }
}
