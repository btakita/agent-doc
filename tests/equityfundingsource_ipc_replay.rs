use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const EFS_LIVE_PROMPT: &str = "Is there an admin for the borrower portal messages? Can we have an email notification whenever a borrower sends a message? Can we support an email response back into the chat? Can the borrower receive emails whenever an admin responds with a message?";

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn equityfundingsource_doc_content() -> String {
    concat!(
        "---\n",
        "agent_doc_session: equityfundingsource-replay\n",
        "agent: codex\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "codex_args: -s danger-full-access\n",
        "prompt_presets:\n",
        "  '#spec-test-commit-push-deploy': update spec + tests. commit + push + deploy\n",
        "queue_active: false\n",
        "---\n\n",
        "## Status\n\n",
        "<!-- agent:status patch=replace -->\n",
        "EFS workers.dev production is deployed; custom-domain cutover is deferred.\n",
        "<!-- /agent:status -->\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\n",
        "*Compacted. Content archived to `.agent-doc/archives/efs-replay.md`*\n\n",
        "- In addition to the Apply for Loan, I need a Borrower Login button.\n",
        "- The footer email should be info@equityfundingsource.com\n",
        "- http://localhost:4200/apply responds with a 404\n\n",
        "#spec-test-commit-push-deploy\n",
        "<!-- agent:boundary:efsreplay -->\n",
        "<!-- /agent:exchange -->\n",
        "###\n",
        "<!--\n",
        "-->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#efsreplay] Deterministic IPC replay fixture.\n",
        "<!-- /agent:backlog -->\n"
    )
    .to_string()
}

fn response_text(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} - gpt-5\n\nHandled {topic}.\n<!-- /patch:exchange -->\n"
    )
}

fn response_heading(topic: &str) -> String {
    format!("### Re: {topic} - gpt-5")
}

fn response_payload_line(topic: &str) -> String {
    format!("Handled {topic}.")
}

fn doc_hash(doc: &Path) -> String {
    let canonical = doc.canonicalize().unwrap();
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())
}

fn init_git_repo(root: &Path, tracked: &Path) {
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["init"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "commit.gpgsign", "false"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .status()
        .unwrap();
}

struct ReplayProject {
    tmp: TempDir,
    doc: PathBuf,
    baseline: PathBuf,
}

impl ReplayProject {
    fn root(&self) -> &Path {
        self.tmp.path()
    }

    fn agent_doc_dir(&self) -> PathBuf {
        self.root().join(".agent-doc")
    }

    fn patches_dir(&self) -> PathBuf {
        self.agent_doc_dir().join("patches")
    }

    fn ack_dir(&self) -> PathBuf {
        self.agent_doc_dir().join("ack-content")
    }

    fn snapshot(&self) -> String {
        fs::read_to_string(
            self.agent_doc_dir()
                .join("snapshots")
                .join(format!("{}.md", doc_hash(&self.doc))),
        )
        .unwrap()
    }

    fn head(&self) -> String {
        let output = ProcessCommand::new("git")
            .current_dir(self.root())
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn visible(&self) -> String {
        fs::read_to_string(&self.doc).unwrap()
    }

    fn type_live_prompt_after_preflight(&self) {
        let current = fs::read_to_string(&self.doc).unwrap();
        let updated = current.replace("<!--\n-->\n", &format!("<!--\n{EFS_LIVE_PROMPT}\n-->\n"));
        assert_ne!(current, updated, "fixture comment shell should be present");
        fs::write(&self.doc, updated).unwrap();
    }
}

fn setup_replay_project(with_patches_dir: bool) -> ReplayProject {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
        "ack-content",
        "claimed-patches",
        "crdt",
        "logs",
        "pre-response",
        "snapshots",
        "state/cycles",
    ] {
        fs::create_dir_all(agent_doc_dir.join(subdir)).unwrap();
    }
    if with_patches_dir {
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
    }

    let doc = tmp.path().join("session.md");
    let original = equityfundingsource_doc_content();
    fs::write(&doc, &original).unwrap();
    init_git_repo(tmp.path(), &doc);

    agent_doc()
        .current_dir(tmp.path())
        .args(["preflight", doc.to_str().unwrap()])
        .assert()
        .success();
    clear_patch_jsons(&agent_doc_dir.join("patches"));

    let baseline = tmp.path().join("baseline.md");
    fs::write(&baseline, &original).unwrap();
    ReplayProject { tmp, doc, baseline }
}

fn clear_patch_jsons(patches_dir: &Path) {
    let Ok(entries) = fs::read_dir(patches_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            fs::remove_file(path).unwrap();
        }
    }
}

fn patch_id(payload: &Value) -> Option<String> {
    payload
        .get("patch_id")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn apply_payload_to_file(payload: &Value, file: &Path) -> Option<String> {
    let before = fs::read_to_string(file).ok()?;
    let patches = payload
        .get("patches")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item
                        .get("component")
                        .or_else(|| item.get("name"))
                        .and_then(Value::as_str)?;
                    let content = item.get("content").and_then(Value::as_str)?;
                    Some(agent_doc::template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(Value::as_str)
        .unwrap_or("");
    let after = agent_doc::template::apply_patches(&before, &patches, unmatched, file).ok()?;
    fs::write(file, &after).ok()?;
    Some(after)
}

fn run_finalize(project: &ReplayProject, topic: &str, code: i32, extra_args: &[&str]) {
    let mut args = vec![
        "finalize",
        project.doc.to_str().unwrap(),
        "--baseline-file",
        project.baseline.to_str().unwrap(),
        "--stream",
    ];
    args.extend_from_slice(extra_args);
    agent_doc()
        .current_dir(project.root())
        .args(args)
        .write_stdin(response_text(topic))
        .assert()
        .code(code);
}

fn assert_live_prompt_visible_but_uncommitted(project: &ReplayProject, topic: &str) {
    let heading = response_heading(topic);
    let body = response_payload_line(topic);
    let visible = project.visible();
    assert!(
        visible.contains(&heading) && visible.contains(&body),
        "response missing from visible file:\n{visible}"
    );
    assert!(
        visible.contains(EFS_LIVE_PROMPT),
        "live EFS prompt should remain visible for the next cycle:\n{visible}"
    );
    assert_eq!(
        visible.matches(EFS_LIVE_PROMPT).count(),
        1,
        "live EFS prompt should not duplicate:\n{visible}"
    );

    let head = project.head();
    assert!(
        head.contains(&heading) && head.contains(&body),
        "response missing from HEAD:\n{head}"
    );
    assert!(
        !head.contains(EFS_LIVE_PROMPT),
        "live EFS prompt typed after preflight must not be committed:\n{head}"
    );

    let snapshot = project.snapshot();
    assert!(
        snapshot.contains(&heading) && snapshot.contains(&body),
        "response missing from snapshot:\n{snapshot}"
    );
    assert!(
        !snapshot.contains(EFS_LIVE_PROMPT),
        "snapshot must stay on content_ours and leave the live EFS prompt for the next cycle:\n{snapshot}"
    );
}

fn assert_no_patch_jsons(project: &ReplayProject) {
    let entries = fs::read_dir(project.patches_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    assert!(
        entries.is_empty(),
        "stale patch JSONs should not be left for a later editor watcher: {entries:?}"
    );
}

#[test]
fn equityfundingsource_socket_ipc_replays_typing_during_finalize() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let listener_root = project.root().to_path_buf();
    let doc_for_listener = project.doc.clone();
    let ack_dir = project.ack_dir();
    let seen_for_listener = seen_payload.clone();
    let server = std::thread::spawn(move || {
        agent_doc_orchestration::ipc_socket::start_listener(&listener_root, move |msg| {
            let payload: Value = serde_json::from_str(msg).ok()?;
            let Some(id) = patch_id(&payload) else {
                return Some(serde_json::json!({"type": "ack"}).to_string());
            };
            let after_apply = apply_payload_to_file(&payload, &doc_for_listener)?;
            fs::write(ack_dir.join(format!("{id}.md")), &after_apply).ok()?;
            *seen_for_listener.lock().ok()? = Some(payload);
            Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
        })
        .ok();
    });
    for _ in 0..100 {
        if agent_doc_orchestration::ipc_socket::is_listener_active(project.root()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        agent_doc_orchestration::ipc_socket::is_listener_active(project.root()),
        "fake socket listener did not start"
    );

    run_finalize(&project, "EFS socket replay", 0, &[]);

    let _ = fs::remove_file(agent_doc_orchestration::ipc_socket::socket_path(project.root()));
    drop(server);

    let payload = seen_payload
        .lock()
        .unwrap()
        .clone()
        .expect("socket listener should capture IPC payload");
    assert!(
        payload.get("fullContent").is_none(),
        "EFS socket replay must stay component-scoped: {payload}"
    );
    assert_live_prompt_visible_but_uncommitted(&project, "EFS socket replay");
    assert_no_patch_jsons(&project);
}

#[test]
fn equityfundingsource_file_ipc_ack_sidecar_replays_typing_during_finalize() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    let patches_dir = project.patches_dir();
    let ack_dir = project.ack_dir();
    let doc_for_watcher = project.doc.clone();
    let seen_ack = Arc::new(Mutex::new(None::<String>));
    let seen_ack_for_watcher = seen_ack.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            let Ok(entries) = fs::read_dir(&patches_dir) else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap();
                let payload: Value = serde_json::from_str(&text).unwrap();
                let Some(id) = patch_id(&payload) else {
                    continue;
                };
                let after_apply = apply_payload_to_file(&payload, &doc_for_watcher).unwrap();
                fs::write(ack_dir.join(format!("{id}.md")), &after_apply).unwrap();
                *seen_ack_for_watcher.lock().unwrap() = Some(after_apply);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    run_finalize(&project, "EFS file IPC replay", 0, &[]);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let ack_content = seen_ack
        .lock()
        .unwrap()
        .clone()
        .expect("watcher should capture ACK sidecar content");
    assert!(
        ack_content.contains(EFS_LIVE_PROMPT),
        "ACK sidecar should model the editor-visible buffer with live typing:\n{ack_content}"
    );
    assert_live_prompt_visible_but_uncommitted(&project, "EFS file IPC replay");
}

#[test]
fn equityfundingsource_stale_patch_replay_is_claimed_before_late_editor_pickup() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    run_finalize(&project, "EFS stale patch replay", 75, &[]);

    assert_live_prompt_visible_but_uncommitted(&project, "EFS stale patch replay");
    assert_no_patch_jsons(&project);
    let claimed = fs::read_dir(project.agent_doc_dir().join("claimed-patches"))
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(
        !claimed.is_empty(),
        "timeout fallback should claim stale patch ids before a late watcher can replay them"
    );
}

#[test]
fn equityfundingsource_force_disk_finalize_replays_typing_without_ipc() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    run_finalize(&project, "EFS direct disk replay", 0, &["--force-disk"]);

    assert_live_prompt_visible_but_uncommitted(&project, "EFS direct disk replay");
    assert_no_patch_jsons(&project);
}
