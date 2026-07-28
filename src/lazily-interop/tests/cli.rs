use std::process::Command;

use serde_json::Value;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_lazily-interop")
}

#[test]
fn feature_group_runs_only_scenario_peers_without_false_staging() {
    let output = Command::new(binary())
        .args(["run", "--format", "json", "--require-channels"])
        .output()
        .expect("run orchestrator");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON result");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["peers"], serde_json::json!(["left", "right"]));
    assert_eq!(report["staged_peers"], serde_json::json!([]));
    assert_eq!(report["coverage"]["peers_started"], 2);
    assert_eq!(report["coverage"]["frames_emitted"], 2);
}

#[test]
fn transport_matrix_includes_every_supported_peer_and_stages_csharp() {
    let output = Command::new(binary())
        .args([
            "run",
            "--registry",
            "tests/fixtures/channel-feature-gate-peers.toml",
            "--scenario",
            "tests/fixtures/channel-feature-gate.json",
            "--format",
            "json",
            "--require-channels",
        ])
        .output()
        .expect("run transport matrix");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON result");
    let cells = report["coverage"]["transport_cells"]
        .as_array()
        .expect("transport cells");

    assert_eq!(report["status"], "pass");
    assert_eq!(
        report["peers"],
        serde_json::json!(["left", "middle", "right"])
    );
    assert_eq!(report["staged_peers"][0]["peer"], "lazily-cs");
    assert_eq!(cells.len(), 30);
    for peer in ["left", "middle", "right"] {
        assert!(cells.iter().any(|cell| cell["sender"] == peer));
        assert!(cells.iter().any(|cell| cell["receiver"] == peer));
    }
    assert!(
        cells
            .iter()
            .all(|cell| cell["bytes_sent"] == cell["bytes_received"])
    );
}

#[test]
fn all_required_mutants_are_caught() {
    let output = Command::new(binary())
        .args(["mutation-check", "--json"])
        .output()
        .expect("run mutation check");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let proofs: Vec<Value> = serde_json::from_slice(&output.stdout).expect("mutation JSON");
    assert_eq!(proofs.len(), 11);
    assert!(proofs.iter().all(|proof| proof["caught"] == true));
}

#[test]
fn one_stdlib_implementation_is_insufficient_and_other_peers_are_staged() {
    let output = Command::new(binary())
        .args([
            "run",
            "--registry",
            "tests/fixtures/stdlib-feature-gate-peers.toml",
            "--scenario",
            "tests/fixtures/stdlib-feature-gate.json",
            "--format",
            "json",
        ])
        .output()
        .expect("run stdlib feature gate");
    assert!(
        !output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON result");
    assert_eq!(report["status"], "fail");
    assert_eq!(
        report["failures"][0]["category"],
        "insufficient_eligible_peers"
    );
    assert_eq!(report["peers"], serde_json::json!(["rust"]));
    assert_eq!(report["staged_peers"][0]["peer"], "staged");
    assert!(
        report["staged_peers"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("stdlib_timer_v1"))
    );
}
