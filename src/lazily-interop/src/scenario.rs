use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::peer::{PeerProcess, validate_transport_receive};
use crate::schema::Schemas;
use crate::types::{
    Coverage, ErrorCategory, Failure, PeerConfig, Plane, Registry, RunReport, RunStatus, Scenario,
    ScenarioStep, StagedPeer, TransportCell,
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
    _require_channels: bool,
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
            add_staged(
                &mut staged,
                &scenario_peer.name,
                &format!("missing required codec: {codec}"),
            );
            let _ = peer.close(schemas);
            continue;
        }
        if let Some(channel) = scenario
            .required_channels
            .iter()
            .find(|channel| !peer.capabilities().channels.contains(channel))
        {
            add_staged(
                &mut staged,
                &scenario_peer.name,
                &format!("missing required channel: {channel}"),
            );
            let _ = peer.close(schemas);
            continue;
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
            let transport_run = coverage.channels_run > 0;
            let mut report = RunReport::pass(
                &scenario.name,
                scenario.seed,
                active_names,
                coverage,
                replay,
            );
            if transport_run {
                report.plane = Plane::Transport;
            }
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
            ScenarioStep::Transport {
                from,
                to,
                channel,
                variant,
                codec,
                frame_set,
                fault,
                bidirectional,
            } => {
                let frames = scenario.frame_sets.get(frame_set).ok_or_else(|| {
                    Failure::new(
                        ErrorCategory::PeerProtocol,
                        Plane::Harness,
                        "orchestrator",
                        format!("transport step references unknown frame_set {frame_set:?}"),
                    )
                })?;
                transport_direction(
                    scenario,
                    peers,
                    schemas,
                    coverage,
                    from,
                    to,
                    channel,
                    variant,
                    variant,
                    codec,
                    frames,
                    fault.as_deref(),
                )?;
                if *bidirectional {
                    transport_direction(
                        scenario,
                        peers,
                        schemas,
                        coverage,
                        to,
                        from,
                        channel,
                        variant,
                        variant,
                        codec,
                        frames,
                        fault.as_deref(),
                    )?;
                }
            }
            ScenarioStep::TransportMatrix {
                channel,
                codec,
                frame_set,
                fault,
            } => {
                let frames = scenario.frame_sets.get(frame_set).ok_or_else(|| {
                    Failure::new(
                        ErrorCategory::PeerProtocol,
                        Plane::Harness,
                        "orchestrator",
                        format!("transport matrix references unknown frame_set {frame_set:?}"),
                    )
                })?;
                let names = peers.keys().cloned().collect::<Vec<_>>();
                for sender_name in &names {
                    for receiver_name in &names {
                        if sender_name == receiver_name {
                            continue;
                        }
                        let sender_variant =
                            advertised_variant(peers, sender_name, channel, codec)?.to_owned();
                        let receiver_variant =
                            advertised_variant(peers, receiver_name, channel, codec)?.to_owned();
                        transport_direction(
                            scenario,
                            peers,
                            schemas,
                            coverage,
                            sender_name,
                            receiver_name,
                            channel,
                            &sender_variant,
                            &receiver_variant,
                            codec,
                            frames,
                            fault.as_deref(),
                        )?;
                    }
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

#[allow(clippy::too_many_arguments)]
fn transport_direction(
    scenario: &Scenario,
    peers: &mut BTreeMap<String, PeerProcess>,
    schemas: &Schemas,
    coverage: &mut Coverage,
    sender_name: &str,
    receiver_name: &str,
    channel: &str,
    sender_variant: &str,
    receiver_variant: &str,
    codec: &str,
    frames: &[Value],
    fault: Option<&str>,
) -> Result<(), Failure> {
    for (name, variant) in [
        (sender_name, sender_variant),
        (receiver_name, receiver_variant),
    ] {
        let peer = peers.get(name).ok_or_else(|| {
            Failure::new(
                ErrorCategory::InsufficientEligiblePeers,
                Plane::Harness,
                name,
                "transport step targets a staged peer",
            )
        })?;
        let variants = peer
            .capabilities()
            .channel_variants
            .get(channel)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if !variants.iter().any(|candidate| candidate == variant) {
            return Err(Failure::new(
                ErrorCategory::MissingRequiredChannel,
                Plane::Transport,
                name,
                format!("channel {channel:?} does not advertise variant {variant:?}"),
            )
            .with_channel_codec(channel, codec));
        }
    }
    for frame in frames {
        schemas
            .validate_transport_frame(frame, sender_name)
            .map_err(|failure| failure.with_channel_codec(channel, codec))?;
    }

    let link = coverage.channels_run as u64 + 1;
    let endpoint_dir =
        std::env::temp_dir().join(format!("lazily-interop-{}-{}", std::process::id(), link));
    fs::create_dir_all(&endpoint_dir).map_err(|error| {
        Failure::new(
            ErrorCategory::PeerLaunch,
            Plane::Transport,
            "orchestrator",
            format!("create channel endpoint directory: {error}"),
        )
    })?;
    let endpoint = endpoint_dir.join("link.sock");
    let receiver_options = fault.map_or_else(
        || json!({"variant": receiver_variant}),
        |fault| json!({"variant": receiver_variant, "fault": fault}),
    );
    let sender_options = fault.map_or_else(
        || json!({"variant": sender_variant}),
        |fault| json!({"variant": sender_variant, "fault": fault}),
    );

    peer_mut(peers, receiver_name)?
        .request_ok(
            &json!({
                "cmd": "link_open",
                "link": link,
                "channel": channel,
                "codec": codec,
                "role": "listen",
                "endpoint": endpoint.display().to_string(),
                "options": receiver_options
            }),
            schemas,
        )
        .map_err(|failure| failure.with_channel_codec(channel, codec))?;
    peer_mut(peers, sender_name)?
        .request_ok(
            &json!({
                "cmd": "link_open",
                "link": link,
                "channel": channel,
                "codec": codec,
                "role": "connect",
                "endpoint": endpoint.display().to_string(),
                "options": sender_options
            }),
            schemas,
        )
        .map_err(|failure| failure.with_channel_codec(channel, codec))?;

    let mut bytes_sent = 0usize;
    for frame in frames {
        let reply = peer_mut(peers, sender_name)?
            .request_ok(
                &json!({"cmd": "link_send", "link": link, "frame": frame}),
                schemas,
            )
            .map_err(|failure| failure.with_channel_codec(channel, codec))?;
        bytes_sent += reply.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize;
    }
    let reply = peer_mut(peers, receiver_name)?
        .request_ok(
            &json!({
                "cmd": "link_recv",
                "link": link,
                "timeout_ms": 2_000,
                "expect": frames.len()
            }),
            schemas,
        )
        .map_err(|failure| failure.with_channel_codec(channel, codec))?;
    validate_transport_receive(receiver_name, frames, &reply)
        .map_err(|failure| failure.with_channel_codec(channel, codec))?;
    let bytes_received = reply.get("bytes").and_then(Value::as_u64).unwrap_or(0) as usize;
    if bytes_sent == 0 || bytes_received != bytes_sent {
        return Err(Failure::new(
            ErrorCategory::FrameTruncated,
            Plane::Transport,
            receiver_name,
            format!("wire byte mismatch: sent {bytes_sent}, received {bytes_received}"),
        )
        .with_channel_codec(channel, codec));
    }

    let _ = peer_mut(peers, sender_name)?
        .request_ok(&json!({"cmd": "link_close", "link": link}), schemas);
    let _ = peer_mut(peers, receiver_name)?
        .request_ok(&json!({"cmd": "link_close", "link": link}), schemas);
    let _ = fs::remove_file(&endpoint);
    let _ = fs::remove_dir(&endpoint_dir);
    coverage.channels_run += 1;
    coverage.transport_cells.push(TransportCell {
        sender: sender_name.to_owned(),
        receiver: receiver_name.to_owned(),
        direction: format!("{sender_name}->{receiver_name}"),
        profile: channel_profile(channel).to_owned(),
        plane: Plane::Transport,
        channel: channel.to_owned(),
        variant: if sender_variant == receiver_variant {
            sender_variant.to_owned()
        } else {
            format!("{sender_variant}->{receiver_variant}")
        },
        codec: codec.to_owned(),
        frame_family: "canonical_ipc".to_owned(),
        scenario: scenario.name.clone(),
        seed: scenario.seed,
        status: RunStatus::Pass,
        unsupported_reason: None,
        bytes_sent,
        bytes_received,
        first_divergence: None,
    });
    Ok(())
}

fn advertised_variant<'a>(
    peers: &'a BTreeMap<String, PeerProcess>,
    peer_name: &str,
    channel: &str,
    codec: &str,
) -> Result<&'a str, Failure> {
    peers
        .get(peer_name)
        .and_then(|peer| peer.capabilities().channel_variants.get(channel))
        .and_then(|variants| variants.first())
        .map(String::as_str)
        .ok_or_else(|| {
            Failure::new(
                ErrorCategory::MissingRequiredChannel,
                Plane::Transport,
                peer_name,
                format!("channel {channel:?} has no advertised production variant"),
            )
            .with_channel_codec(channel, codec)
        })
}

fn channel_profile(channel: &str) -> &'static str {
    match channel {
        "in_process_ffi" | "shared_memory" | "ipc_socket" => "native",
        "websocket" => "browser",
        "webrtc_datachannel" => "portable",
        _ => "portable",
    }
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
