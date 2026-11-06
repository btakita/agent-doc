//! Record a quota block from a harness subagent-end (`#workerquotaautodetect`).
//!
//! The coordinator used to be the only writer, via `queue report-complete` or
//! `queue report-quota-blocked`. A subagent the harness ends without that text
//! stayed `Running` until its TTL and then went `Lost`. This scan marks the
//! bound attempt `QuotaBlocked` from the worker id alone.
//!
//! Roots are the `.agent-doc` projects of the hook paths, plus the main
//! checkout of a linked git worktree when that checkout has its own project.
//! The hook cwd is not recorded as the worker's worktree.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_doc_queue::queue_claim::{
    QUEUE_CLAIMS_STATE_KIND, QueueClaim, QueueClaimLedger, QueueClaimState,
};
use agent_doc_queue::worker_quota::{QuotaAutodetectChoice, WorkerQuotaBlock};
use anyhow::Result;

use crate::queue_claim::{block_attempt_on_quota, claim_clock_now_secs, load_ledger};

/// A claim this scan moved to, or refreshed as, quota-blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentQuotaBlockApplied {
    pub file: PathBuf,
    pub claim: QueueClaim,
    /// `false` when this worker was already blocked with the same evidence and
    /// harness. The ledger refresh stays; the hook stays quiet.
    pub announce: bool,
}

/// What the scan did with one worker id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentQuotaAutodetect {
    Applied(SubagentQuotaBlockApplied),
    /// No Running, Lost, or QuotaBlocked attempt is bound to this worker.
    NoMatch,
    /// Two attempts of the same rank started together. Nothing was written.
    Ambiguous,
}

#[derive(Clone)]
struct Hit {
    file: PathBuf,
    item_text: String,
    generation: u64,
    state: QueueClaimState,
    started_at_secs: u64,
    owner_harness: Option<String>,
    prior_evidence: Option<String>,
    prior_harness: Option<String>,
}

/// Mark the attempt bound to `worker_id` quota-blocked.
///
/// `scan_paths` are hook paths (`cwd`, workspace root, transcript). A linked
/// worktree with no `.agent-doc` of its own still reaches the main checkout's
/// ledger. `preferred_doc` is the session document the parent hook state names;
/// when that ledger has the worker, other documents are not consulted.
pub fn apply_subagent_quota_autodetect(
    scan_paths: &[PathBuf],
    preferred_doc: Option<&Path>,
    worker_id: &str,
    block: WorkerQuotaBlock,
) -> Result<SubagentQuotaAutodetect> {
    let worker_id = worker_id.trim();
    if worker_id.is_empty() {
        return Ok(SubagentQuotaAutodetect::NoMatch);
    }
    let mut hits = collect_hits(&quota_scan_roots(scan_paths), worker_id);
    if let Some(doc) = preferred_doc {
        consider_document(&mut hits, doc, worker_id);
        let preferred = hits
            .iter()
            .filter(|hit| same_file(&hit.file, doc))
            .cloned()
            .collect::<Vec<_>>();
        match choose(&preferred) {
            QuotaAutodetectChoice::Selected(index) => {
                return apply_hit(&preferred[index], block);
            }
            QuotaAutodetectChoice::Ambiguous => {
                return Ok(SubagentQuotaAutodetect::Ambiguous);
            }
            QuotaAutodetectChoice::None => {}
        }
    }
    match choose(&hits) {
        QuotaAutodetectChoice::Selected(index) => apply_hit(&hits[index], block),
        QuotaAutodetectChoice::Ambiguous => Ok(SubagentQuotaAutodetect::Ambiguous),
        QuotaAutodetectChoice::None => Ok(SubagentQuotaAutodetect::NoMatch),
    }
}

fn choose(hits: &[Hit]) -> QuotaAutodetectChoice {
    let rows = hits
        .iter()
        .map(|hit| (hit.state, hit.started_at_secs))
        .collect::<Vec<_>>();
    agent_doc_queue::worker_quota::choose_quota_autodetect_claim(&rows)
}

fn apply_hit(hit: &Hit, mut block: WorkerQuotaBlock) -> Result<SubagentQuotaAutodetect> {
    let harness = block.harness.clone().or_else(|| hit.owner_harness.clone());
    block.harness = harness.map(|name| agent_doc_harness::normalize_harness_name(&name));
    block.blocked_at_secs = claim_clock_now_secs();
    let announce = !(hit.state == QueueClaimState::QuotaBlocked
        && hit.prior_evidence.as_deref() == Some(block.evidence.as_str())
        && hit.prior_harness == block.harness);
    let claim = block_attempt_on_quota(&hit.file, &hit.item_text, Some(hit.generation), block)?;
    Ok(SubagentQuotaAutodetect::Applied(
        SubagentQuotaBlockApplied {
            file: hit.file.clone(),
            claim,
            announce,
        },
    ))
}

fn collect_hits(roots: &[PathBuf], worker_id: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let db = agent_doc_sqlite::state_store::state_db_path(root);
        if !db.exists() {
            continue;
        }
        // `#testisolationleaks`: a cargo test binary must not open a live
        // project's state.db. Production has no scope, so this is a no-op there.
        if !agent_doc_fs::path_in_test_runtime_scope(&db) {
            continue;
        }
        let conn = match agent_doc_sqlite::state_store::open_state_db(root) {
            Ok(conn) => conn,
            Err(err) => {
                eprintln!(
                    "[agent-doc] subagent-end quota autodetect could not open {}: {err:#}",
                    db.display()
                );
                continue;
            }
        };
        let records = match agent_doc_sqlite::state_store::list_queue_document_state_from_db(
            &conn,
            QUEUE_CLAIMS_STATE_KIND,
        ) {
            Ok(records) => records,
            Err(err) => {
                eprintln!(
                    "[agent-doc] subagent-end quota autodetect could not list claims in {}: {err:#}",
                    root.display()
                );
                continue;
            }
        };
        for record in records {
            if record.canonical_path.is_empty() {
                continue;
            }
            let file = PathBuf::from(&record.canonical_path);
            if !mark_seen(&mut seen, &file) {
                continue;
            }
            let ledger = match QueueClaimLedger::from_state_payload(&record.payload_json) {
                Ok(ledger) => ledger,
                Err(err) => {
                    eprintln!(
                        "[agent-doc] subagent-end quota autodetect skipped {}: {err:#}",
                        file.display()
                    );
                    continue;
                }
            };
            push_hits(&mut hits, &file, &ledger, worker_id);
        }
    }
    hits
}

fn consider_document(hits: &mut Vec<Hit>, doc: &Path, worker_id: &str) {
    if hits.iter().any(|hit| same_file(&hit.file, doc)) {
        return;
    }
    let Ok(ledger) = load_ledger(doc) else {
        return;
    };
    push_hits(hits, doc, &ledger, worker_id);
}

fn push_hits(hits: &mut Vec<Hit>, file: &Path, ledger: &QueueClaimLedger, worker_id: &str) {
    for claim in &ledger.claims {
        let Some(worker) = claim.worker.as_ref() else {
            continue;
        };
        if worker.worker_id != worker_id {
            continue;
        }
        let block = claim.quota_block.as_ref();
        hits.push(Hit {
            file: file.to_path_buf(),
            item_text: claim.item_text.clone(),
            generation: claim.generation,
            state: claim.state,
            started_at_secs: worker.started_at_secs,
            owner_harness: claim
                .owner_session
                .as_ref()
                .and_then(|session| session.harness.clone()),
            prior_evidence: block.map(|block| block.evidence.clone()),
            prior_harness: block.and_then(|block| block.harness.clone()),
        });
    }
}

fn mark_seen(seen: &mut HashSet<PathBuf>, file: &Path) -> bool {
    let key = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    seen.insert(key)
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// `.agent-doc` roots to scan for `paths`, including a linked worktree's main
/// checkout when that checkout is itself an agent-doc project.
pub fn quota_scan_roots(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for path in paths {
        consider_project(&mut roots, path);
        if let Some(main) = linked_worktree_main(path) {
            consider_project(&mut roots, &main);
        }
        if let Some(git_root) = git_worktree_root(path)
            && git_root.join(".agent-doc").is_dir()
        {
            consider_project(&mut roots, &git_root);
        }
    }
    roots
}

fn consider_project(roots: &mut Vec<PathBuf>, path: &Path) {
    if !path.join(".agent-doc").is_dir() && agent_doc_fs::find_project_root(path).is_none() {
        return;
    }
    let Some(root) = agent_doc_fs::find_project_root(path) else {
        return;
    };
    if !root.join(".agent-doc").is_dir() {
        return;
    }
    let root = root.canonicalize().unwrap_or(root);
    if !agent_doc_fs::path_in_test_runtime_scope(&root) {
        return;
    }
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn git_worktree_root(path: &Path) -> Option<PathBuf> {
    let git = git_dotgit(path)?;
    git.parent().map(Path::to_path_buf)
}

fn linked_worktree_main(path: &Path) -> Option<PathBuf> {
    let git = git_dotgit(path)?;
    if !git.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&git).ok()?;
    let gitdir = text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
    })?;
    let gitdir = PathBuf::from(gitdir);
    let gitdir = if gitdir.is_absolute() {
        gitdir
    } else {
        git.parent()?.join(gitdir)
    };
    let worktrees = gitdir.parent()?;
    if worktrees.file_name() != Some(std::ffi::OsStr::new("worktrees")) {
        return None;
    }
    worktrees.parent()?.parent().map(Path::to_path_buf)
}

fn git_dotgit(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_file() {
        path.parent().map(Path::to_path_buf)
    } else {
        Some(path.to_path_buf())
    };
    while let Some(dir) = current {
        let git = dir.join(".git");
        if git.exists() {
            return Some(git);
        }
        current = dir.parent().map(Path::to_path_buf);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::store_ledger_for_test;
    use agent_doc_queue::queue_claim::{QueueClaimLedger, QueueClaimState};
    use agent_doc_queue::worker_quota::WorkerQuotaBlock;

    fn project(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "# task\n").unwrap();
        doc
    }

    fn block() -> WorkerQuotaBlock {
        WorkerQuotaBlock {
            harness: Some("claude".to_string()),
            limit: "session limit".to_string(),
            evidence: "You've hit your session limit · resets 9pm".to_string(),
            reset_hint: Some("resets 9pm".to_string()),
            worktree: None,
            branch: None,
            blocked_at_secs: 0,
        }
    }

    fn running(item: &str, worker: &str) -> QueueClaimLedger {
        let mut ledger = QueueClaimLedger::default();
        assert_eq!(
            ledger.claim(item, "subagent:sdk", 1_000, 100),
            agent_doc_queue::queue_claim::ClaimOutcome::Created
        );
        ledger
            .start(item, "subagent:sdk", worker, 1_000, 100, Some(1))
            .expect("start");
        ledger
    }

    #[test]
    fn a_running_worker_is_blocked_without_a_coordinator_report() {
        let dir = tempfile::tempdir().unwrap();
        let doc = project(dir.path());
        store_ledger_for_test(&doc, &running("finish sdk", "worker-sdk")).unwrap();
        let applied = match apply_subagent_quota_autodetect(
            &[dir.path().to_path_buf()],
            None,
            "worker-sdk",
            block(),
        )
        .unwrap()
        {
            SubagentQuotaAutodetect::Applied(applied) => applied,
            other => panic!("expected a block, got {other:?}"),
        };
        assert!(applied.announce);
        assert_eq!(applied.claim.state, QueueClaimState::QuotaBlocked);
        assert_eq!(
            applied
                .claim
                .quota_block
                .as_ref()
                .unwrap()
                .harness
                .as_deref(),
            Some("claude-code")
        );
        assert!(
            applied
                .claim
                .quota_block
                .as_ref()
                .unwrap()
                .worktree
                .is_none()
        );
    }

    #[test]
    fn an_unknown_worker_and_a_reported_attempt_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let doc = project(dir.path());
        let mut ledger = running("finish sdk", "worker-sdk");
        ledger.report("finish sdk", Some(1)).unwrap();
        store_ledger_for_test(&doc, &ledger).unwrap();
        assert_eq!(
            apply_subagent_quota_autodetect(
                &[dir.path().to_path_buf()],
                None,
                "someone-else",
                block()
            )
            .unwrap(),
            SubagentQuotaAutodetect::NoMatch
        );
        assert_eq!(
            apply_subagent_quota_autodetect(
                &[dir.path().to_path_buf()],
                None,
                "worker-sdk",
                block()
            )
            .unwrap(),
            SubagentQuotaAutodetect::NoMatch
        );
        assert_eq!(
            load_ledger(&doc).unwrap().claims[0].state,
            QueueClaimState::Reported
        );
    }

    #[test]
    fn a_lost_attempt_is_absorbed_and_a_repeat_stays_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let doc = project(dir.path());
        let mut ledger = running("finish sdk", "worker-sdk");
        ledger.claims[0].state = QueueClaimState::Lost;
        store_ledger_for_test(&doc, &ledger).unwrap();
        let first = match apply_subagent_quota_autodetect(
            &[dir.path().to_path_buf()],
            None,
            "worker-sdk",
            block(),
        )
        .unwrap()
        {
            SubagentQuotaAutodetect::Applied(applied) => applied,
            other => panic!("expected a block, got {other:?}"),
        };
        assert!(first.announce);
        assert_eq!(first.claim.state, QueueClaimState::QuotaBlocked);
        let again = match apply_subagent_quota_autodetect(
            &[dir.path().to_path_buf()],
            None,
            "worker-sdk",
            block(),
        )
        .unwrap()
        {
            SubagentQuotaAutodetect::Applied(applied) => applied,
            other => panic!("expected a refresh, got {other:?}"),
        };
        assert!(!again.announce);
        assert_eq!(
            again.claim.quota_block.as_ref().unwrap().blocked_at_secs,
            first.claim.quota_block.as_ref().unwrap().blocked_at_secs
        );
    }

    #[test]
    fn a_linked_worktree_without_a_project_scans_the_main_checkout() {
        let main = tempfile::tempdir().unwrap();
        let doc = project(main.path());
        store_ledger_for_test(&doc, &running("finish sdk", "worker-sdk")).unwrap();
        let worker = tempfile::tempdir().unwrap();
        let gitdir = main.path().join(".git/worktrees/wt");
        std::fs::write(
            worker.path().join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        let applied = match apply_subagent_quota_autodetect(
            &[worker.path().to_path_buf()],
            None,
            "worker-sdk",
            block(),
        )
        .unwrap()
        {
            SubagentQuotaAutodetect::Applied(applied) => applied,
            other => panic!("expected the main checkout's claim, got {other:?}"),
        };
        assert_eq!(applied.claim.state, QueueClaimState::QuotaBlocked);
        assert!(same_file(&applied.file, &doc));
    }
}
