use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::peer::PeerProcess;
use crate::schema::Schemas;
use crate::types::{
    Coverage, ErrorCategory, Failure, PeerConfig, Plane, Registry, RunReport, Scenario,
    ScenarioStep, StagedPeer,
};

pub fn load_registry(path: &Path, self_exe: &Path) -> Result<Registry> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read peer registry {}", path.display()))?;
    let mut registry: Registry =
        toml::from_str(&text).with_context(|| format!("parse peer registry {}", path.display()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    for config in registry.peers.values_mut() {
        for argument in &mut config.command {
            if argument == "{self}" {
                *argument = self_exe.display().to_string();
            }
        }
        if let Some(cwd) = &config.cwd
            && cwd.is_relative()
        {
            config.cwd = Some(base.join(cwd));
        }
    }
    Ok(registry)
}

pub fn load_scenario(path: &Path, schemas: &Schemas) -> Result<Scenario> {
    let bytes = fs::read(path).with_context(|| format!("read scenario {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse scenario JSON {}", path.display()))?;
    schemas
        .validate_scenario(&value, path)
        .map_err(anyhow::Error::new)?;
    serde_json::from_value(value).with_context(|| format!("decode scenario {}", path.display()))
}

pub fn run_scenario(
    scenario: &Scenario,
    registry: &Registry,
    schemas: &Schemas,
    timeout: Duration,
    require_channels: bool,
    replay: String,
) -> RunReport {
    let requested_names = scenario
        .peers
        .iter()
        .map(|peer| peer.name.clone())
        .collect::<Vec<_>>();
    let mut coverage = Coverage::default();
    let mut staged = registry
        .peers
        .iter()
        .filter(|(_, config)| config.staged)
        .map(|(name, _)| StagedPeer {
            peer: name.clone(),
            reason: "registry entry is staged".to_owned(),
        })
        .collect::<Vec<_>>();
    let mut peers = BTreeMap::<String, PeerProcess>::new();

    for scenario_peer in &scenario.peers {
        let Some(config) = registry.peers.get(&scenario_peer.name) else {
            return failed(
                scenario,
                requested_names,
                coverage,
                staged,
                replay,
                Failure::new(
                    ErrorCategory::PeerLaunch,
                    Plane::Harness,
                    &scenario_peer.name,
                    "scenario peer is absent from registry",
                ),
            );
        };
        if config.staged {
            add_staged(&mut staged, &scenario_peer.name, "registry entry is staged");
            continue;
        }
        let peer = match PeerProcess::start(
            &scenario_peer.name,
            config,
            scenario_peer.peer_id,
            timeout,
            schemas,
        ) {
            Ok(peer) => peer,
            Err(failure) => {
                return failed(scenario, requested_names, coverage, staged, replay, failure);
            }
        };

        let missing_features = scenario
            .required_features
            .iter()
            .filter(|feature| !peer.capabilities().features.contains(feature))
            .cloned()
            .collect::<Vec<_>>();
        if !missing_features.is_empty() {
            add_staged(
                &mut staged,
                &scenario_peer.name,
                &format!("missing features: {}", missing_features.join(", ")),
            );
            let _ = peer.close(schemas);
            continue;
        }

        if let Some(codec) = scenario
            .required_codecs
            .iter()
            .find(|codec| !peer.capabilities().codecs.contains(codec))
        {
            return failed(
                scenario,
                requested_names,
                coverage,
                staged,
                replay,
                Failure::new(
                    ErrorCategory::MissingRequiredCodec,
                    Plane::Harness,
                    &scenario_peer.name,
                    format!("eligible peer does not support required codec {codec:?}"),
                ),
            );
        }
        if require_channels
            && let Some(channel) = scenario
                .required_channels
                .iter()
                .find(|channel| !peer.capabilities().channels.contains(channel))
        {
            return failed(
                scenario,
                requested_names,
                coverage,
                staged,
                replay,
                Failure::new(
                    ErrorCategory::MissingRequiredChannel,
                    Plane::Transport,
                    &scenario_peer.name,
                    format!("eligible peer does not support required channel {channel:?}"),
                ),
            );
        }
        coverage.peers_started += 1;
        peers.insert(scenario_peer.name.clone(), peer);
    }

    let active_names = peers.keys().cloned().collect::<Vec<_>>();
    if peers.len() < 2 {
        return failed(
            scenario,
            active_names,
            coverage,
            staged,
            replay,
            Failure::new(
                ErrorCategory::InsufficientEligiblePeers,
                Plane::Harness,
                "orchestrator",
                format!(
                    "feature group has {} active peer(s), need at least 2",
                    peers.len()
                ),
            ),
        );
    }

    let execution = execute_steps(scenario, &mut peers, schemas, &mut coverage);
    let close_failure = close_all(peers, schemas);
    let failure = execution.err().or(close_failure);
    match failure {
        Some(failure) => failed(scenario, active_names, coverage, staged, replay, failure),
        None => {
            let mut report = RunReport::pass(
                &scenario.name,
                scenario.seed,
                active_names,
                coverage,
                replay,
            );
            report.staged_peers = staged;
            report
        }
    }
}

fn execute_steps(
    scenario: &Scenario,
    peers: &mut BTreeMap<String, PeerProcess>,
    schemas: &Schemas,
    coverage: &mut Coverage,
) -> Result<(), Failure> {
    let mut frames = BTreeMap::<String, Value>::new();
    for step in &scenario.steps {
        match step {
            ScenarioStep::LocalSet {
                peer,
                node,
                key,
                state,
                at,
                save_as,
            } => {
                let process = peer_mut(peers, peer)?;
                let reply = process.request_ok(
                    &json!({
                        "cmd": "local_set",
                        "node": node,
                        "key": key,
                        "state": state,
                        "at": at
                    }),
                    schemas,
                )?;
                let frame = reply.get("frame").cloned().ok_or_else(|| {
                    Failure::new(
                        ErrorCategory::PeerProtocol,
                        Plane::Semantic,
                        peer,
                        "local_set response has no frame",
                    )
                })?;
                schemas.validate_frame(&frame, peer)?;
                let emitted_state = frame.pointer("/CrdtSync/ops/0/state");
                if emitted_state != Some(state) {
                    return Err(Failure::new(
                        ErrorCategory::FrameTruncated,
                        Plane::Semantic,
                        peer,
                        "local_set frame state differs from the requested state",
                    ));
                }
                coverage.frames_emitted += 1;
                frames.insert(save_as.clone(), frame);
            }
            ScenarioStep::Deliver {
                frame,
                to,
                at,
                expect_applied,
            } => {
                let value = frames.get(frame).ok_or_else(|| {
                    Failure::new(
                        ErrorCategory::PeerProtocol,
                        Plane::Harness,
                        "orchestrator",
                        format!("delivery references unknown frame {frame:?}"),
                    )
                })?;
                for destination in to {
                    let process = peer_mut(peers, destination)?;
                    let reply = process.request_ok(
                        &json!({"cmd": "deliver", "frame": value, "at": at}),
                        schemas,
                    )?;
                    let applied = reply
                        .get("applied")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            Failure::new(
                                ErrorCategory::PeerProtocol,
                                Plane::Semantic,
                                destination,
                                "deliver response has no integer applied count",
                            )
                        })? as usize;
                    if applied != *expect_applied {
                        let category = if *expect_applied == 0 {
                            ErrorCategory::DuplicateApplied
                        } else {
                            ErrorCategory::Convergence
                        };
                        return Err(Failure::new(
                            category,
                            Plane::Semantic,
                            destination,
                            format!(
                                "deliver reported applied={applied}, expected {expect_applied}"
                            ),
                        ));
                    }
                    coverage.frames_delivered += 1;
                }
            }
            ScenarioStep::AssertConverged => {
                for (name, process) in peers.iter_mut() {
                    let reply = process.request_ok(&json!({"cmd": "snapshot"}), schemas)?;
                    let cells = reply
                        .get("cells")
                        .and_then(Value::as_array)
                        .cloned()
                        .ok_or_else(|| {
                            Failure::new(
                                ErrorCategory::PeerProtocol,
                                Plane::Semantic,
                                name,
                                "snapshot response has no cells array",
                            )
                        })?;
                    if cells != scenario.expected_snapshot {
                        return Err(Failure::new(
                            ErrorCategory::Convergence,
                            Plane::Semantic,
                            name,
                            "snapshot differs from scenario expected_snapshot",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn peer_mut<'a>(
    peers: &'a mut BTreeMap<String, PeerProcess>,
    name: &str,
) -> Result<&'a mut PeerProcess, Failure> {
    peers.get_mut(name).ok_or_else(|| {
        Failure::new(
            ErrorCategory::InsufficientEligiblePeers,
            Plane::Harness,
            name,
            "scenario step targets a peer outside the eligible feature group",
        )
    })
}

fn close_all(peers: BTreeMap<String, PeerProcess>, schemas: &Schemas) -> Option<Failure> {
    peers
        .into_values()
        .filter_map(|peer| peer.close(schemas).err())
        .next()
}

fn failed(
    scenario: &Scenario,
    peers: Vec<String>,
    coverage: Coverage,
    staged_peers: Vec<StagedPeer>,
    replay: String,
    failure: Failure,
) -> RunReport {
    let mut report = RunReport::fail(
        &scenario.name,
        scenario.seed,
        peers,
        coverage,
        replay,
        failure,
    );
    report.staged_peers = staged_peers;
    report
}

fn add_staged(staged: &mut Vec<StagedPeer>, peer: &str, reason: &str) {
    if staged.iter().any(|entry| entry.peer == peer) {
        return;
    }
    staged.push(StagedPeer {
        peer: peer.to_owned(),
        reason: reason.to_owned(),
    });
}

pub fn resolve_default_paths(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        root.join("peers.toml"),
        root.join("scenarios/reference-convergence.json"),
        root.join("../lazily-spec"),
    )
}

pub fn peer_config(command: Vec<String>, mutation: Option<&str>) -> PeerConfig {
    let mut env = BTreeMap::new();
    if let Some(mutation) = mutation {
        env.insert("LAZILY_INTEROP_MUTATION".to_owned(), mutation.to_owned());
    }
    PeerConfig {
        command,
        cwd: None,
        env,
        staged: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_placeholder_expands() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("peers.toml");
        fs::write(
            &path,
            "[peers.a]\ncommand = [\"{self}\", \"reference-peer\"]\n",
        )
        .unwrap();
        let registry = load_registry(&path, Path::new("/tmp/orchestrator")).unwrap();
        assert_eq!(registry.peers["a"].command[0], "/tmp/orchestrator");
    }
}
