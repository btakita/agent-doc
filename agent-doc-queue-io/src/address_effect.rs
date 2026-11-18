//! Remote observation for `#address` completion (`#addressproof`).
//!
//! The queue crate decides. This module only reads the pull request. A fixture
//! owner (`sample-org`) is never sent to the network. GitHub observation is off
//! until the process enables it, so unit tests stay offline and fail closed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use agent_doc_queue::address_completion_proof::{
    AddressEffectFacts, AddressEffectGuard, AddressObservation, AddressSnapshot, AddressTarget,
    address_review_target, forced_observation,
};
use serde::Deserialize;

static GITHUB: AtomicBool = AtomicBool::new(false);
static MEMORY: parking_lot::Mutex<BTreeMap<String, AddressSnapshot>> =
    parking_lot::Mutex::new(BTreeMap::new());

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
    let key = format!("{}|{}", file.display(), target.key());
    let baseline = load_baseline(file, &key);
    if baseline.is_none() {
        save_baseline(file, &key, &current);
    }
    AddressObservation::Seen { baseline, current }
}

fn fixture_target(target: &AddressTarget) -> bool {
    target.owner == "sample-org" || target.repo.starts_with("sample")
}

fn address_item_texts(content: &str) -> Vec<String> {
    let mut texts = Vec::new();
    if let Ok(components) = agent_doc_element::element::parse(content) {
        for component in components
            .iter()
            .filter(|component| component.name == "queue")
        {
            let Ok(entries) = agent_doc_queue::document_queue::parse(component.content(content))
            else {
                continue;
            };
            for prompt in agent_doc_queue::document_queue::prompts(&entries) {
                texts.push(prompt.text.clone());
            }
        }
    }
    if texts.is_empty() {
        texts.extend(
            content
                .lines()
                .filter(|line| line.contains("#address"))
                .map(ToString::to_string),
        );
    }
    texts
}

fn baseline_path(file: &Path) -> Option<PathBuf> {
    let root = agent_doc_fs::find_project_root(file)?;
    Some(root.join(".agent-doc/address-effect-baselines.json"))
}

fn load_baseline(file: &Path, key: &str) -> Option<AddressSnapshot> {
    if let Some(snapshot) = MEMORY.lock().get(key).cloned() {
        return Some(snapshot);
    }
    let path = baseline_path(file)?;
    let text = std::fs::read_to_string(path).ok()?;
    let entries: BTreeMap<String, AddressSnapshot> = serde_json::from_str(&text).ok()?;
    let snapshot = entries.get(key)?.clone();
    MEMORY.lock().insert(key.to_string(), snapshot.clone());
    Some(snapshot)
}

fn save_baseline(file: &Path, key: &str, snapshot: &AddressSnapshot) {
    MEMORY.lock().insert(key.to_string(), snapshot.clone());
    let Some(path) = baseline_path(file) else {
        return;
    };
    let mut entries = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<BTreeMap<String, AddressSnapshot>>(&text).ok())
        .unwrap_or_default();
    entries.insert(key.to_string(), snapshot.clone());
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&entries) {
        let _ = std::fs::write(path, text);
    }
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
    let comments = parse_comments(&comments_json);
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

fn parse_comments(stdout: &str) -> Vec<GhComment> {
    if let Ok(comments) = serde_json::from_str::<Vec<GhComment>>(stdout) {
        return comments;
    }
    let mut all = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(comments) = serde_json::from_str::<Vec<GhComment>>(line) {
            all.extend(comments);
        }
    }
    all
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
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stdout);
                }
                return status.success().then_some(stdout);
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
