use std::fs;
use std::process::Command;

use serde_json::Value;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_lazily-interop")
}

#[test]
fn prepare_warms_every_peer_before_the_strict_hello_deadline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = temp.path().join("peers.toml");
    let wrapper = |marker: &str| {
        format!(
            "command = [\"sh\", \"-c\", \"if [ ! -e {marker} ]; then sleep 2; touch {marker}; fi; exec \\\"$1\\\" reference-peer\", \"sh\", \"{{self}}\"]\ncwd = \".\"\n"
        )
    };
    fs::write(
        &registry,
        format!(
            "[peers.left]\n{}\n[peers.right]\n{}",
            wrapper("left.prepared"),
            wrapper("right.prepared")
        ),
    )
    .expect("write registry");

    let prepare = Command::new(binary())
        .args([
            "prepare",
            "--registry",
            registry.to_str().expect("registry path"),
            "--scenario",
            "scenarios/reference-convergence.json",
            "--timeout-ms",
            "5000",
        ])
        .output()
        .expect("prepare peers");
    assert!(
        prepare.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&prepare.stdout),
        String::from_utf8_lossy(&prepare.stderr),
    );
    assert!(temp.path().join("left.prepared").is_file());
    assert!(temp.path().join("right.prepared").is_file());

    let run = Command::new(binary())
        .args([
            "run",
            "--registry",
            registry.to_str().expect("registry path"),
            "--scenario",
            "scenarios/reference-convergence.json",
            "--timeout-ms",
            "1000",
        ])
        .output()
        .expect("run warmed peers");
    assert!(
        run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
}

#[test]
fn network_suite_prepares_the_nine_way_peer_set_before_running_it() {
    let makefile = fs::read_to_string("Makefile").expect("read Makefile");
    assert!(makefile.contains("network-suite: prepare-network-peers"));

    let prepare = makefile
        .find("cargo run --quiet -- prepare --scenario scenarios/nine-way-convergence.json")
        .expect("nine-way preparation command");
    let run = makefile
        .find("cargo run --quiet -- run --scenario scenarios/nine-way-convergence.json")
        .expect("nine-way strict run command");
    assert!(
        prepare < run,
        "peer preparation must precede the strict run"
    );
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

#[test]
fn durable_client_v1_replays_all_portable_proof_dimensions() {
    // The real go/js/py peers may compile or sync on first launch, which is not
    // a protocol answer. Warm them under the preparation budget first, exactly
    // like `make network-suite`; only then enforce the strict hello deadline.
    let prepare = Command::new(binary())
        .args([
            "prepare",
            "--scenario",
            "scenarios/durable-client-v1.json",
            "--timeout-ms",
            "300000",
        ])
        .output()
        .expect("prepare durable client peers");
    assert!(
        prepare.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&prepare.stdout),
        String::from_utf8_lossy(&prepare.stderr),
    );

    let output = Command::new(binary())
        .args([
            "run",
            "--scenario",
            "scenarios/durable-client-v1.json",
            "--format",
            "json",
        ])
        .output()
        .expect("run durable client interop");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON result");
    assert_eq!(report["status"], "pass");
    assert_eq!(
        report["peers"],
        serde_json::json!(["lazily-go", "lazily-js", "lazily-py"])
    );
    assert_eq!(report["coverage"]["feature_steps_run"], 24);
}
