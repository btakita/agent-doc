//! `#flakyreceipthelpers`: the shared simulated editor records receipts on a
//! warm in-process controller and is a known-live editor endpoint.

use agent_doc_test_support::{SimulatedEditorListener, SimulatedEditorReply};
use std::time::Duration;
use tempfile::TempDir;

/// Longer than the production 6s editor receipt deadline.
const SLOWER_THAN_RECEIPT_DEADLINE: Duration = Duration::from_secs(7);

fn applied(patch_id: &str) -> String {
    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id}).to_string()
}

#[test]
fn simulated_editor_records_receipt_on_the_warm_controller() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    let doc = root.join("plan.md");
    std::fs::write(&doc, "before\n").unwrap();

    let listener =
        agent_doc_test_support::start_live_prompt_drift_receipt_listener(&root, "after\n".into());
    let response = agent_doc_ipc_io::send_message(
        &root,
        &serde_json::json!({
            "type": "refresh_content",
            "patch_id": "simulated-editor-receipt",
            "file": doc.to_string_lossy(),
        }),
    )
    .unwrap()
    .expect("the simulated editor must answer");
    assert!(
        response.contains("\"applied\""),
        "a recorded receipt is acknowledged as applied: {response}"
    );
    assert_eq!(std::fs::read_to_string(&doc).unwrap(), "after\n");
    // Drop asserts every receipt was recorded and no detached controller was
    // cold-launched to record it.
    drop(listener);
}

#[test]
fn simulated_editor_receipt_deadline_is_a_hang_ceiling() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let _listener = SimulatedEditorListener::start(&root, "test_slow_editor", |message| {
        std::thread::sleep(SLOWER_THAN_RECEIPT_DEADLINE);
        let patch_id = message
            .get("patch_id")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        SimulatedEditorReply::Raw(applied(patch_id))
    });
    assert!(agent_doc_ipc_io::known_live_test_editors::serves(&root));

    let response = agent_doc_ipc_io::send_message(
        &root,
        &serde_json::json!({"type": "refresh_content", "patch_id": "slow"}),
    )
    .expect("a slow answer from a known-live simulated editor is host load, not absence")
    .expect("the simulated editor must answer");
    assert!(response.contains("\"applied\""), "{response}");
}

#[test]
fn unregistered_editor_keeps_the_production_receipt_deadline() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    let listener_root = root.clone();
    std::thread::spawn(move || {
        let _ = agent_doc_ipc_io::start_listener(&listener_root, |_| {
            std::thread::sleep(SLOWER_THAN_RECEIPT_DEADLINE);
            Some(applied("slow"))
        });
    });
    agent_doc_test_support::wait_for_live_prompt_drift_listener(&root);
    assert!(!agent_doc_ipc_io::known_live_test_editors::serves(&root));

    let err = agent_doc_ipc_io::send_message(
        &root,
        &serde_json::json!({"type": "refresh_content", "patch_id": "slow"}),
    )
    .expect_err("an ordinary listener must still time out at the receipt deadline");
    assert!(
        format!("{err:#}").contains("IPC receipt timeout"),
        "unexpected error: {err:#}"
    );
}
