//! Preflight settlement of external blocker ids (`#blockergraph`).
//!
//! Each turn, rebuild the decision-graph links for stays-open ids that are
//! not tracked rows. A linked GitHub pull request or issue that is closed, or
//! a decision that names the id, settles that id. The result is the document
//! marker `<!-- agent-doc:blocker-settled ... -->`, which the queue derivation
//! reads. A failed lookup leaves the gate in place.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use agent_doc_answer_ledger::extract::observe;
use agent_doc_answer_ledger::graph::{
    DecisionGraph, GithubUpstream, GithubUpstreamKind, merge_settlement_marker,
};
use anyhow::Result;

/// Settle external blockers already evidenced in `file`. Best-effort: a
/// failure changes nothing and never blocks admission.
pub fn run_blocker_settlement_maintenance(file: &Path) -> Result<bool> {
    let source = "preflight_blocker_settlement";
    let content = match super::current_text_via_preflight_authority_retrying(file, source)? {
        Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
        Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
            match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(_) => return Ok(false),
            }
        }
        Some(_) => return Ok(false),
    };
    settle_blockers_in_content(file, &content, None).map(|persisted| persisted.is_some())
}

/// `#q-e9483473162d`: blocker settlement as one step of the detached admission
/// reconcile, fenced to the admitted cut `content`. Each unsettled upstream
/// costs a `gh` lookup of up to 15s, so it never runs on the admission hook.
/// Returns the persisted document when a settlement marker was written.
pub fn run_blocker_settlement_maintenance_on_admitted_cut(
    file: &Path,
    content: &str,
    fence: &dyn super::PendingMaintenanceEffectFence,
) -> Result<Option<String>> {
    settle_blockers_in_content(file, content, Some(fence))
}

fn settle_blockers_in_content(
    file: &Path,
    content: &str,
    fence: Option<&dyn super::PendingMaintenanceEffectFence>,
) -> Result<Option<String>> {
    let source = "preflight_blocker_settlement";
    let observations = observe(content, &[]);
    let mut graph = DecisionGraph::default();
    graph.link_external_blockers(&observations, content);
    let upstreams = graph.blocker_upstreams();
    let root = agent_doc_project_root_io::project_root_containing(file);
    let closed = closed_upstreams(root.as_deref(), &upstreams);
    let settled = graph.settled_external_blocker_ids(&closed);
    let Some(next) = merge_settlement_marker(content, &settled) else {
        return Ok(None);
    };
    if let Some(fence) = fence {
        // The lookups above can take seconds; prove the cut is still current
        // before the write rather than only at the compare-and-swap.
        fence.check(file)?;
    }
    let admitted = fence.map(|_| agent_doc_hash::content_hash(content));
    let persisted = super::persist_queue_maintenance_doc_with_admitted_authority(
        file,
        &next,
        content,
        root.as_deref(),
        source,
        admitted.as_deref(),
        fence,
        None,
    )?;
    let listed = settled.into_iter().collect::<Vec<_>>().join(" ");
    eprintln!("[preflight] blocker settlement: {listed} (#blockergraph)");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "blocker_settlement_applied file={} ids={listed}",
            file.display()
        ),
    );
    Ok(Some(persisted))
}

fn closed_upstreams(root: Option<&Path>, upstreams: &[GithubUpstream]) -> Vec<GithubUpstream> {
    let cache_path = root.map(|root| root.join(".agent-doc/state/blocker-upstreams.txt"));
    let cached = cache_path
        .as_ref()
        .map(|path| read_cache(path))
        .unwrap_or_default();
    let mut closed = Vec::new();
    let mut newly_closed = Vec::new();
    for upstream in upstreams {
        let key = upstream.node_id();
        if cached.contains(&key) {
            closed.push(upstream.clone());
            continue;
        }
        if let Some(true) = github_is_closed(upstream) {
            newly_closed.push(key);
            closed.push(upstream.clone());
        }
    }
    if let Some(path) = cache_path.as_ref()
        && !newly_closed.is_empty()
    {
        remember_closed(path, &newly_closed);
    }
    closed
}

fn read_cache(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn remember_closed(path: &Path, keys: &[String]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing = read_cache(path);
    let before = existing.len();
    existing.extend(keys.iter().cloned());
    if existing.len() == before {
        return;
    }
    let body = existing.into_iter().collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(path, body + "\n");
}

fn github_is_closed(upstream: &GithubUpstream) -> Option<bool> {
    let kind = match upstream.kind {
        GithubUpstreamKind::Issue => "issue",
        GithubUpstreamKind::PullRequest => "pr",
    };
    let repo = format!("{}/{}", upstream.owner, upstream.repo);
    let output = Command::new("timeout")
        .args([
            "15",
            "gh",
            kind,
            "view",
            &upstream.number.to_string(),
            "--repo",
            &repo,
            "--json",
            "state",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    if text.contains("\"merged\"") || text.contains("\"closed\"") {
        Some(true)
    } else if text.contains("\"open\"") {
        Some(false)
    } else {
        None
    }
}
