//! Remote observation for `#address` completion (`#addressproof`).
//!
//! The queue crate decides. This module only reads the pull request. A fixture
//! owner (`sample-org`) is never sent to the network. GitHub observation is off
//! until the process enables it, so unit tests stay offline and fail closed.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use agent_doc_queue::address_completion_proof::{
    AddressEffectFacts, AddressEffectGuard, AddressObservation, AddressSnapshot, AddressTarget,
    address_review_target, forced_observation,
};
use agent_doc_sqlite::state_store;
use anyhow::{Context, Result};
use serde::Deserialize;

static GITHUB: AtomicBool = AtomicBool::new(false);

/// Allow this process to read pull requests with `gh`.
pub fn enable_github_observation() {
    GITHUB.store(true, Ordering::Relaxed);
}

pub fn github_enabled() -> bool {
    GITHUB.load(Ordering::Relaxed)
}

/// The observation `report-complete` uses for `item`.
///
/// A [`forced_observation`] wins. Otherwise this reads GitHub when the process
/// enabled it and the item names a non-fixture pull request.
pub fn observation_for_item(file: &Path, item: &str) -> AddressObservation {
    if let Some(observation) = forced_observation() {
        return observation;
    }
    let Some(target) = address_review_target(item) else {
        return AddressObservation::Unseen;
    };
    observation_for_target(file, &target)
}

/// Capture at an execution ingress. Report and closeout only observe.
pub fn capture_before_execution(file: &Path, item: &str) -> Result<()> {
    if !github_enabled() {
        return Ok(());
    }
    // Direct-run submission adds a context tree after the selected lineage.
    // A sibling review in that tree must not choose the execution target.
    let item = [
        agent_doc_queue::document_queue::LATEST_REPLY_LABEL,
        agent_doc_queue::document_queue::PROMPT_TREE_HEADER,
    ]
    .iter()
    .filter_map(|label| item.find(&format!("\n{label}")))
    .min()
    .map_or(item, |at| &item[..at]);
    let Some(target) = address_review_target(item).filter(|target| !fixture_target(target)) else {
        return Ok(());
    };
    ensure_dispatch_baseline(file, &target, fetch_snapshot).with_context(|| {
        format!("could not capture the pull-request baseline before dispatch for {item:?} (#addressproof)")
    })
}

fn ensure_dispatch_baseline(
    file: &Path,
    target: &AddressTarget,
    fetch: impl FnOnce(&AddressTarget) -> Option<AddressSnapshot>,
) -> Result<()> {
    if load_baseline(file, target)?.is_none() {
        let snapshot = fetch(target).context("GitHub did not return a pull-request snapshot")?;
        save_baseline(file, target, &snapshot)?;
    }
    Ok(())
}

/// Install observations for address rows in `content`, when this process reads
/// GitHub or a test forced one observation. Returns `None` when there is
/// nothing to install, leaving any guard the caller already installed in place.
pub fn install_for_content(file: &Path, content: &str) -> Option<AddressEffectGuard> {
    if forced_observation().is_none() && !github_enabled() {
        return None;
    }
    let mut facts = AddressEffectFacts::default();
    for text in address_item_texts(content) {
        let Some(target) = address_review_target(&text) else {
            continue;
        };
        facts
            .by_key
            .entry(target.key())
            .or_insert_with(|| observation_for_target(file, &target));
    }
    if facts.by_key.is_empty() {
        return None;
    }
    Some(AddressEffectGuard::install(facts))
}

fn observation_for_target(file: &Path, target: &AddressTarget) -> AddressObservation {
    if let Some(observation) = forced_observation() {
        return observation;
    }
    if !github_enabled() || fixture_target(target) {
        return AddressObservation::Unseen;
    }
    let Some(current) = fetch_snapshot(target) else {
        return AddressObservation::Unseen;
    };
    observe_snapshot(file, target, current)
}

fn observe_snapshot(
    file: &Path,
    target: &AddressTarget,
    current: AddressSnapshot,
) -> AddressObservation {
    let baseline = match load_baseline(file, target) {
        Ok(baseline) => baseline,
        Err(error) => {
            eprintln!("[queue] could not read pull-request baseline: {error:#} (#addressproof)");
            return AddressObservation::Unseen;
        }
    };
    AddressObservation::Seen { baseline, current }
}

fn fixture_target(target: &AddressTarget) -> bool {
    target.owner == "sample-org" || target.repo.starts_with("sample")
}

fn address_item_texts(content: &str) -> Vec<String> {
    agent_doc_queue::queue_continuation::all_queue_head_texts(content).unwrap_or_default()
}

fn baseline_kind(target: &AddressTarget) -> String {
    format!("address_effect_baseline:{}", target.key())
}

fn load_baseline(file: &Path, target: &AddressTarget) -> Result<Option<AddressSnapshot>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    if state_store::state_db_path(&root).exists() {
        let conn = state_store::open_state_db_read_only(&root)?;
        let document_hash = agent_doc_hash::path_hash(file)?;
        if state_store::state_table_exists(&conn, "queue_document_state")?
            && let Some(record) = state_store::load_queue_document_state_from_db(
                &conn,
                &document_hash,
                &baseline_kind(target),
            )?
        {
            return Ok(Some(serde_json::from_str(&record.payload_json)?));
        }
    }
    let legacy = root.join(".agent-doc/address-effect-baselines.json");
    let text = match std::fs::read_to_string(&legacy) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read legacy pull-request baselines"),
    };
    let entries: BTreeMap<String, AddressSnapshot> = serde_json::from_str(&text)?;
    let canonical = file.canonicalize()?;
    Ok(entries
        .get(&format!("{}|{}", canonical.display(), target.key()))
        .or_else(|| entries.get(&format!("{}|{}", file.display(), target.key())))
        .cloned())
}

fn save_baseline(file: &Path, target: &AddressTarget, snapshot: &AddressSnapshot) -> Result<()> {
    let root = agent_doc_fs::find_project_root(file).context("no agent-doc project root")?;
    let conn = state_store::open_state_db(&root)?;
    // The first read owns the baseline. Concurrent captures of a second
    // target or the same target must not erase an earlier observation.
    conn.execute(
        "INSERT INTO queue_document_state
         (document_hash, state_kind, canonical_path, payload_json, updated_at_secs)
         VALUES (?1, ?2, ?3, ?4, CAST(strftime('%s', 'now') AS INTEGER))
         ON CONFLICT(document_hash, state_kind) DO NOTHING",
        [
            agent_doc_hash::path_hash(file)?,
            baseline_kind(target),
            file.canonicalize()?.to_string_lossy().into_owned(),
            serde_json::to_string(snapshot)?,
        ],
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct HeadView {
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

#[derive(Debug, Deserialize)]
struct GhComment {
    id: u64,
    #[serde(default)]
    in_reply_to_id: Option<u64>,
    #[serde(default)]
    commit_id: Option<String>,
    #[serde(default)]
    pull_request_review_id: Option<u64>,
}

fn fetch_snapshot(target: &AddressTarget) -> Option<AddressSnapshot> {
    let repo = format!("{}/{}", target.owner, target.repo);
    let number = target.number.to_string();
    let head_json = gh(
        &[
            "pr",
            "view",
            number.as_str(),
            "--repo",
            repo.as_str(),
            "--json",
            "headRefOid",
        ],
        Duration::from_secs(8),
    )?;
    let head: HeadView = serde_json::from_str(&head_json).ok()?;
    let api = format!("repos/{repo}/pulls/{number}/comments");
    let comments_json = gh(&["api", "--paginate", api.as_str()], Duration::from_secs(8))?;
    let comments = parse_comments(&comments_json)?;
    let reply_ids = reply_ids(target, &comments);
    let comment_commit = target.discussion_id.and_then(|id| {
        comments
            .iter()
            .find(|comment| comment.id == id)
            .and_then(|comment| comment.commit_id.clone())
    });
    Some(AddressSnapshot {
        head_sha: head.head_ref_oid,
        reply_ids,
        comment_commit,
        thread_named: target.thread_named(),
    })
}

fn reply_ids(target: &AddressTarget, comments: &[GhComment]) -> Vec<u64> {
    let mut ids: Vec<u64> = comments
        .iter()
        .filter(|comment| match (target.discussion_id, target.review_id) {
            (Some(discussion), _) => comment.in_reply_to_id == Some(discussion),
            (None, Some(review)) => {
                comment.pull_request_review_id == Some(review) && comment.in_reply_to_id.is_some()
            }
            (None, None) => comment.in_reply_to_id.is_some(),
        })
        .map(|comment| comment.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn parse_comments(stdout: &str) -> Option<Vec<GhComment>> {
    if let Ok(comments) = serde_json::from_str::<Vec<GhComment>>(stdout) {
        return Some(comments);
    }
    let mut all = Vec::new();
    for page in serde_json::Deserializer::from_str(stdout).into_iter::<Vec<GhComment>>() {
        all.extend(page.ok()?);
    }
    (!stdout.trim().is_empty()).then_some(all)
}

fn gh(args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    // Read while gh runs. Waiting for exit before draining stdout deadlocks
    // a full pipe on a PR with enough discussion comments.
    let reader = std::thread::spawn(move || {
        let mut stdout = String::new();
        pipe.read_to_string(&mut stdout).map(|_| stdout)
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = reader.join().ok()?.ok()?;
                return status.success().then_some(stdout);
            }
            Ok(None) if started.elapsed() <= timeout => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Ok(None) | Err(_) => {
                if let Err(error) = child.kill() {
                    eprintln!("[queue] could not stop GitHub observation: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("[queue] could not reap GitHub observation: {error}");
                }
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::address_completion_proof::{
        AddressCompletion, AddressProof, judge_address_completion,
    };

    #[test]
    fn nested_address_uses_the_exact_parent_and_a_report_does_not_seed_its_baseline() {
        let content = concat!(
            "<!-- agent:run id=\"main\" subagents -->\n",
            "- ### https://github.com/sample-org/sample-repo/pull/11\n",
            "  - #address\n",
            "- ### https://github.com/sample-org/sample-repo/pull/12\n",
            "  - #address\n",
            "<!-- /agent:run -->\n",
        );
        let targets = address_item_texts(content)
            .iter()
            .filter_map(|item| address_review_target(item))
            .collect::<Vec<_>>();
        assert_eq!(
            targets
                .iter()
                .map(|target| target.number)
                .collect::<Vec<_>>(),
            [11, 12],
        );

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, content).unwrap();
        let snapshot = |head: &str, reply_ids| AddressSnapshot {
            head_sha: head.to_string(),
            reply_ids,
            comment_commit: None,
            thread_named: false,
        };
        let before = snapshot("fa3a3f449986d565f21aeb6e0f792323bc8d1d58", vec![]);
        let after = snapshot("31b6dd51595a26bc1c7d1c052a1ad8d614a949ff9", vec![23]);
        assert_eq!(
            judge_address_completion(&observe_snapshot(&file, &targets[0], after.clone()), None,),
            AddressCompletion::NoEffect {
                head: after.head_sha.clone(),
            },
        );
        assert_eq!(load_baseline(&file, &targets[0]).unwrap(), None);

        ensure_dispatch_baseline(&file, &targets[0], |_| Some(before.clone())).unwrap();
        save_baseline(&file, &targets[1], &after).unwrap();
        save_baseline(&file, &targets[0], &after).unwrap();
        assert_eq!(
            load_baseline(&file, &targets[1]).unwrap(),
            Some(after.clone())
        );
        assert_eq!(
            load_baseline(&file, &targets[0]).unwrap(),
            Some(before.clone())
        );
        assert_eq!(
            judge_address_completion(&observe_snapshot(&file, &targets[0], after.clone()), None,),
            AddressCompletion::Proven(AddressProof::HeadChanged {
                from: before.head_sha,
                to: after.head_sha,
            }),
        );
    }

    #[test]
    fn reply_ids_follow_the_named_discussion() {
        let target = AddressTarget {
            owner: "sample-org".to_string(),
            repo: "sample-repo".to_string(),
            number: 302,
            discussion_id: Some(10),
            review_id: None,
        };
        let comments = vec![
            GhComment {
                id: 10,
                in_reply_to_id: None,
                commit_id: Some("abc".to_string()),
                pull_request_review_id: None,
            },
            GhComment {
                id: 11,
                in_reply_to_id: Some(10),
                commit_id: None,
                pull_request_review_id: None,
            },
            GhComment {
                id: 12,
                in_reply_to_id: Some(99),
                commit_id: None,
                pull_request_review_id: None,
            },
        ];
        assert_eq!(reply_ids(&target, &comments), vec![11]);
    }

    #[test]
    fn a_fixture_target_is_not_sent_to_the_network() {
        let target = address_review_target(
            "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r1",
        )
        .expect("target");
        assert!(fixture_target(&target));
        assert!(matches!(
            observation_for_target(Path::new("session.md"), &target),
            AddressObservation::Unseen
        ));
    }
}
