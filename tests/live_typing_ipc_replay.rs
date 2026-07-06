use agent_doc_hash::content_hash;
use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const LIVE_TYPING_PROMPT: &str = "Does the portal message center have admin notifications? Can the operator reply from email and have the reply appear in the portal chat? Can portal users receive an email when the operator responds?";
const TEST_EDITOR_ID: &str = "jetbrains-test-editor";

fn agent_doc() -> Command {
    cargo_bin_cmd!("agent-doc")
}

fn live_typing_doc_content() -> String {
    concat!(
        "---\n",
        "agent_doc_session: live-typing-replay\n",
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
        "The portal test fixture is deployed; custom-domain cutover is deferred.\n",
        "<!-- /agent:status -->\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\n",
        "*Compacted. Content archived to `.agent-doc/archives/live-typing-replay.md`*\n\n",
        "- In addition to the apply form, I need an account login button.\n",
        "- The footer email should use the shared support mailbox.\n",
        "- http://localhost:4200/apply responds with a 404\n\n",
        "#spec-test-commit-push-deploy\n",
        "<!-- agent:boundary:livetype -->\n",
        "<!-- /agent:exchange -->\n",
        "###\n",
        "<!--\n",
        "-->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#livetype] Deterministic IPC replay fixture.\n",
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
    content_hash(canonical.to_string_lossy().as_ref())
}

fn record_operator_buffer(file: &Path, content: &str) {
    let file_key = file.to_string_lossy();
    agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
        file_key.as_ref(),
        content,
        TEST_EDITOR_ID,
        "jetbrains",
        "test",
        &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
    )
    .unwrap();
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
        let updated = current.replace("<!--\n-->\n", &format!("<!--\n{LIVE_TYPING_PROMPT}\n-->\n"));
        assert_ne!(current, updated, "fixture comment shell should be present");
        fs::write(&self.doc, &updated).unwrap();
        record_operator_buffer(&self.doc, &updated);
    }
}

fn setup_replay_project(with_patches_dir: bool) -> ReplayProject {
    let tmp = TempDir::new().unwrap();
    let agent_doc_dir = tmp.path().join(".agent-doc");
    for subdir in [
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
    let original = live_typing_doc_content();
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
                    Some(agent_doc_template::PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(Value::as_str)
        .unwrap_or("");
    let after = agent_doc_template_io::apply_patches(&before, &patches, unmatched, file).ok()?;
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
    let mut cmd = agent_doc();
    cmd.current_dir(project.root());
    if code == 0 {
        cmd.env("AGENT_DOC_FILE_IPC_TIMEOUT_MS", "8000");
    }
    cmd.args(args)
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
        visible.contains(LIVE_TYPING_PROMPT),
        "live prompt should remain visible for the next cycle:\n{visible}"
    );
    assert_eq!(
        visible.matches(LIVE_TYPING_PROMPT).count(),
        1,
        "live prompt should not duplicate:\n{visible}"
    );

    let head = project.head();
    assert!(
        head.contains(&heading) && head.contains(&body),
        "response missing from HEAD:\n{head}"
    );
    assert!(
        !head.contains(LIVE_TYPING_PROMPT),
        "live prompt typed after preflight must not be committed:\n{head}"
    );

    let snapshot = project.snapshot();
    assert!(
        snapshot.contains(&heading) && snapshot.contains(&body),
        "response missing from snapshot:\n{snapshot}"
    );
    assert!(
        !snapshot.contains(LIVE_TYPING_PROMPT),
        "snapshot must stay on content_ours and leave the live prompt for the next cycle:\n{snapshot}"
    );
}

fn assert_live_prompt_visible_and_committed(project: &ReplayProject, topic: &str) {
    let heading = response_heading(topic);
    let body = response_payload_line(topic);
    let visible = project.visible();
    assert!(
        visible.contains(&heading) && visible.contains(&body),
        "response missing from visible file:\n{visible}"
    );
    assert_eq!(
        visible.matches(LIVE_TYPING_PROMPT).count(),
        1,
        "live prompt should remain visible exactly once:\n{visible}"
    );

    let head = project.head();
    assert!(
        head.contains(&heading) && head.contains(&body),
        "response missing from HEAD:\n{head}"
    );
    assert_eq!(
        head.matches(LIVE_TYPING_PROMPT).count(),
        1,
        "lazily-proven live prompt should be committed exactly once:\n{head}"
    );

    let snapshot = project.snapshot();
    assert!(
        snapshot.contains(&heading) && snapshot.contains(&body),
        "response missing from snapshot:\n{snapshot}"
    );
    assert_eq!(
        snapshot.matches(LIVE_TYPING_PROMPT).count(),
        1,
        "lazily-proven snapshot should preserve the live prompt exactly once:\n{snapshot}"
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

fn patch_jsons(project: &ReplayProject) -> Vec<PathBuf> {
    fs::read_dir(project.patches_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect()
}

#[test]
fn socket_ipc_replays_live_typing_during_finalize() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    let seen_payload = Arc::new(Mutex::new(None::<Value>));
    let listener_root = project.root().to_path_buf();
    let doc_for_listener = project.doc.clone();
    let seen_for_listener = seen_payload.clone();
    let server = std::thread::spawn(move || {
        agent_doc_ipc_io::start_listener(&listener_root, move |msg| {
            let payload: Value = serde_json::from_str(msg).ok()?;
            let Some(id) = patch_id(&payload) else {
                return Some(serde_json::json!({"type": "receipt", "status": "applied"}).to_string());
            };
            let after_apply = apply_payload_to_file(&payload, &doc_for_listener)?;
            record_operator_buffer(&doc_for_listener, &after_apply);
            agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                &doc_for_listener,
                &id,
                &after_apply,
                "socket_ipc_test",
            )
            .ok()?;
            *seen_for_listener.lock().ok()? = Some(payload);
            Some(serde_json::json!({"type": "receipt", "status": "applied", "id": id}).to_string())
        })
        .ok();
    });
    for _ in 0..100 {
        if agent_doc_ipc_io::is_listener_active(project.root()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        agent_doc_ipc_io::is_listener_active(project.root()),
        "fake socket listener did not start"
    );

    run_finalize(&project, "socket live typing replay", 0, &[]);

    let _ = fs::remove_file(agent_doc_ipc_io::socket_path(project.root()));
    drop(server);

    let payload = seen_payload
        .lock()
        .unwrap()
        .clone()
        .expect("socket listener should capture IPC payload");
    assert!(
        payload.get("fullContent").is_none(),
        "socket live typing replay must stay component-scoped: {payload}"
    );
    assert_live_prompt_visible_and_committed(&project, "socket live typing replay");
    assert_no_patch_jsons(&project);
}

#[test]
fn file_ipc_lazily_event_replays_live_typing_during_finalize() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    let patches_dir = project.patches_dir();
    let doc_for_watcher = project.doc.clone();
    let seen_receipt = Arc::new(Mutex::new(None::<String>));
    let seen_receipt_for_watcher = seen_receipt.clone();
    let watcher = std::thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
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
                record_operator_buffer(&doc_for_watcher, &after_apply);
                agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
                    &doc_for_watcher,
                    &id,
                    &after_apply,
                    "file_ipc_test",
                )
                .unwrap();
                *seen_receipt_for_watcher.lock().unwrap() = Some(after_apply);
                fs::remove_file(path).unwrap();
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });

    run_finalize(&project, "file IPC live typing replay", 0, &[]);
    assert!(watcher.join().unwrap(), "file IPC watcher saw no patch");

    let receipt_content = seen_receipt
        .lock()
        .unwrap()
        .clone()
        .expect("watcher should capture lazily receipt content");
    assert!(
        receipt_content.contains(LIVE_TYPING_PROMPT),
        "lazily receipt should model the editor-visible buffer with live typing:\n{receipt_content}"
    );
    assert_live_prompt_visible_and_committed(&project, "file IPC live typing replay");
}

#[test]
fn live_typing_timeout_recovers_detached_response_and_keeps_prompt_uncommitted() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    agent_doc()
        .current_dir(project.root())
        .env("AGENT_DOC_FILE_IPC_TIMEOUT_MS", "50")
        .args([
            "finalize",
            project.doc.to_str().unwrap(),
            "--baseline-file",
            project.baseline.to_str().unwrap(),
            "--stream",
        ])
        .write_stdin(response_text("stale patch live typing replay"))
        .assert()
        .success()
        .stderr(predicates::str::contains(
            "recovering through document authority (detached_disk_authority)",
        ));

    assert_live_prompt_visible_but_uncommitted(&project, "stale patch live typing replay");
    let patches = patch_jsons(&project);
    assert!(
        patches.is_empty(),
        "detached timeout recovery should cancel queued patches"
    );
    let claimed = fs::read_dir(project.agent_doc_dir().join("claimed-patches"))
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert!(
        !claimed.is_empty(),
        "detached timeout recovery should claim cancelled patches"
    );
    let ops_log = fs::read_to_string(project.agent_doc_dir().join("logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("recovery=detached_disk_authority")
            && ops_log.contains("ipc_patch_cancelled_for_document_authority"),
        "IPC timeout should recover through detached document authority:\n{ops_log}"
    );
}

#[test]
fn force_disk_finalize_replays_live_typing_without_ipc() {
    let project = setup_replay_project(true);
    project.type_live_prompt_after_preflight();

    run_finalize(
        &project,
        "direct disk live typing replay",
        0,
        &["--force-disk"],
    );

    assert_live_prompt_visible_but_uncommitted(&project, "direct disk live typing replay");
    assert_no_patch_jsons(&project);
}
