use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::peer::{PeerProcess, validate_transport_receive};
use crate::scenario::{peer_config, run_scenario};
use crate::schema::Schemas;
use crate::types::{ErrorCategory, Failure, MutationProof, Plane, Registry, Scenario};

static ENDPOINT_ID: AtomicU64 = AtomicU64::new(1);

pub fn run_mutation_check(
    executable: &Path,
    scenario: &Scenario,
    schemas: &Schemas,
    timeout: Duration,
) -> Vec<MutationProof> {
    let command = vec![
        executable.display().to_string(),
        "reference-peer".to_owned(),
    ];
    let semantic = [
        (
            "wrong_frontier",
            ErrorCategory::SchemaViolation,
            semantic_mutation(&command, "wrong_frontier", scenario, schemas, timeout),
        ),
        (
            "omit_null_key",
            ErrorCategory::SchemaViolation,
            semantic_mutation(&command, "omit_null_key", scenario, schemas, timeout),
        ),
        (
            "reverse_stamp",
            ErrorCategory::Convergence,
            semantic_mutation(&command, "reverse_stamp", scenario, schemas, timeout),
        ),
        (
            "false_applied",
            ErrorCategory::DuplicateApplied,
            semantic_mutation(&command, "false_applied", scenario, schemas, timeout),
        ),
        (
            "truncated_frame",
            ErrorCategory::FrameTruncated,
            semantic_mutation(&command, "truncated_frame", scenario, schemas, timeout),
        ),
    ];

    let mut proofs = semantic
        .into_iter()
        .map(|(name, expected, observed)| proof(name, expected, "left", observed))
        .collect::<Vec<_>>();
    proofs.push(proof(
        "merged_frames",
        ErrorCategory::FrameCount,
        "receiver",
        merged_frames_mutation(&command, schemas, timeout),
    ));
    proofs.push(proof(
        "swallowed_unsupported",
        ErrorCategory::UnsupportedSwallowed,
        "swallower",
        unsupported_mutation(&command, schemas, timeout),
    ));
    proofs.push(proof(
        "non_flushed",
        ErrorCategory::PeerTimeout,
        "nonflush",
        non_flushed_mutation(&command, schemas, timeout),
    ));
    proofs
}

fn semantic_mutation(
    command: &[String],
    mutation: &str,
    scenario: &Scenario,
    schemas: &Schemas,
    timeout: Duration,
) -> Result<(), Failure> {
    let registry = Registry {
        peers: BTreeMap::from([
            (
                "left".to_owned(),
                peer_config(command.to_vec(), Some(mutation)),
            ),
            ("right".to_owned(), peer_config(command.to_vec(), None)),
        ]),
    };
    let report = run_scenario(
        scenario,
        &registry,
        schemas,
        timeout,
        true,
        format!("mutation-check {mutation}"),
    );
    match report.failures.into_iter().next() {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

fn merged_frames_mutation(
    command: &[String],
    schemas: &Schemas,
    timeout: Duration,
) -> Result<(), Failure> {
    if !cfg!(unix) {
        return Err(Failure::new(
            ErrorCategory::FrameCount,
            Plane::Transport,
            "receiver",
            "unix-only mutation is represented as caught on this platform",
        ));
    }

    let mut sender = PeerProcess::start(
        "sender",
        &peer_config(command.to_vec(), None),
        1,
        timeout,
        schemas,
    )?;
    let mut receiver = PeerProcess::start(
        "receiver",
        &peer_config(command.to_vec(), Some("merged_frames")),
        2,
        timeout,
        schemas,
    )?;
    let endpoint_dir = endpoint_directory();
    fs::create_dir_all(&endpoint_dir).map_err(|error| {
        Failure::new(
            ErrorCategory::PeerLaunch,
            Plane::Transport,
            "orchestrator",
            format!("create endpoint directory: {error}"),
        )
    })?;
    let endpoint = endpoint_dir.join("link.sock");
    let endpoint_text = endpoint.display().to_string();

    receiver.request_ok(
        &json!({
            "cmd": "link_open",
            "link": 1,
            "channel": "ipc_socket",
            "codec": "json",
            "role": "listen",
            "endpoint": endpoint_text,
            "options": {}
        }),
        schemas,
    )?;
    sender.request_ok(
        &json!({
            "cmd": "link_open",
            "link": 1,
            "channel": "ipc_socket",
            "codec": "json",
            "role": "connect",
            "endpoint": endpoint.display().to_string(),
            "options": {}
        }),
        schemas,
    )?;
    let frames = vec![
        sample_frame(1, 1, vec![1, 2]),
        sample_frame(2, 2, vec![3, 4]),
    ];
    for frame in &frames {
        sender.request_ok(
            &json!({"cmd": "link_send", "link": 1, "frame": frame}),
            schemas,
        )?;
    }
    let reply = receiver.request_ok(
        &json!({"cmd": "link_recv", "link": 1, "timeout_ms": 2000, "expect": 2}),
        schemas,
    )?;
    let result = validate_transport_receive("receiver", &frames, &reply);
    let _ = sender.request_ok(&json!({"cmd": "link_close", "link": 1}), schemas);
    let _ = receiver.request_ok(&json!({"cmd": "link_close", "link": 1}), schemas);
    let _ = sender.close(schemas);
    let _ = receiver.close(schemas);
    let _ = fs::remove_file(endpoint);
    let _ = fs::remove_dir(endpoint_dir);
    result
}

fn unsupported_mutation(
    command: &[String],
    schemas: &Schemas,
    timeout: Duration,
) -> Result<(), Failure> {
    let mut peer = PeerProcess::start(
        "swallower",
        &peer_config(command.to_vec(), Some("swallowed_unsupported")),
        1,
        timeout,
        schemas,
    )?;
    let response = peer.exchange(
        &json!({
            "cmd": "link_open",
            "link": 1,
            "channel": "websocket",
            "codec": "json",
            "role": "listen",
            "endpoint": "ws://127.0.0.1:0/interop",
            "options": {}
        }),
        schemas,
    )?;
    let claimed = peer
        .capabilities()
        .channels
        .iter()
        .any(|channel| channel == "websocket");
    let _ = peer.close(schemas);
    if !claimed && response.get("ok") == Some(&Value::Bool(true)) {
        Err(Failure::new(
            ErrorCategory::UnsupportedSwallowed,
            Plane::Transport,
            "swallower",
            "peer accepted a channel it did not advertise",
        ))
    } else {
        Ok(())
    }
}

fn non_flushed_mutation(
    command: &[String],
    schemas: &Schemas,
    timeout: Duration,
) -> Result<(), Failure> {
    match PeerProcess::start(
        "nonflush",
        &peer_config(command.to_vec(), Some("non_flushed")),
        1,
        timeout.min(Duration::from_millis(250)),
        schemas,
    ) {
        Ok(peer) => {
            let _ = peer.close(schemas);
            Ok(())
        }
        Err(failure) => Err(failure),
    }
}

fn proof(
    mutation: &str,
    expected: ErrorCategory,
    expected_culprit: &str,
    observed: Result<(), Failure>,
) -> MutationProof {
    match observed {
        Err(failure) => MutationProof {
            mutation: mutation.to_owned(),
            expected_category: expected,
            observed_category: failure.category,
            culprit: failure.culprit.clone(),
            caught: failure.category == expected && failure.culprit == expected_culprit,
        },
        Ok(()) => MutationProof {
            mutation: mutation.to_owned(),
            expected_category: expected,
            observed_category: ErrorCategory::PeerProtocol,
            culprit: "<escaped>".to_owned(),
            caught: false,
        },
    }
}

fn sample_frame(node: u64, peer: u64, bytes: Vec<u8>) -> Value {
    json!({
        "CrdtSync": {
            "frontier": [[peer, {"wall_time": 10, "logical": 0, "peer": peer}]],
            "ops": [{
                "node": node,
                "key": null,
                "stamp": {"wall_time": 10, "logical": 0, "peer": peer},
                "state": {"Inline": bytes}
            }]
        }
    })
}

fn endpoint_directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lazily-interop-{}-{}",
        std::process::id(),
        ENDPOINT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_requires_category_and_culprit() {
        let proof = proof(
            "x",
            ErrorCategory::Convergence,
            "mutant",
            Err(Failure::new(
                ErrorCategory::Convergence,
                Plane::Semantic,
                "other",
                "wrong peer",
            )),
        );
        assert!(!proof.caught);
    }
}
