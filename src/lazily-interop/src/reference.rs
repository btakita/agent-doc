use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, BufRead, BufWriter, Write};

#[cfg(unix)]
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::PROTOCOL_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mutation {
    None,
    WrongFrontier,
    OmitNullKey,
    ReverseStamp,
    FalseApplied,
    TruncatedFrame,
    MergedFrames,
    SwallowedUnsupported,
    NonFlushed,
    CloseAfterPrefix,
    CorruptPayload,
    ReorderedFrames,
}

impl Mutation {
    fn from_env() -> Self {
        match std::env::var("LAZILY_INTEROP_MUTATION").as_deref() {
            Ok("wrong_frontier") => Self::WrongFrontier,
            Ok("omit_null_key") => Self::OmitNullKey,
            Ok("reverse_stamp") => Self::ReverseStamp,
            Ok("false_applied") => Self::FalseApplied,
            Ok("truncated_frame") => Self::TruncatedFrame,
            Ok("merged_frames") => Self::MergedFrames,
            Ok("swallowed_unsupported") => Self::SwallowedUnsupported,
            Ok("non_flushed") => Self::NonFlushed,
            Ok("close_after_prefix") => Self::CloseAfterPrefix,
            Ok("corrupt_payload") => Self::CorruptPayload,
            Ok("reordered_frames") => Self::ReorderedFrames,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct Stamp {
    wall_time: u64,
    logical: u64,
    peer: u64,
}

#[derive(Debug, Clone)]
struct Cell {
    stamp: Stamp,
    state: Value,
}

struct ReferencePeer {
    peer_id: Option<u64>,
    mutation: Mutation,
    cells: BTreeMap<(u64, Option<String>), Cell>,
    seen: BTreeSet<String>,
    last_stamp: Option<Stamp>,
    feature_observations: BTreeMap<String, Value>,
    #[cfg(unix)]
    links: HashMap<u64, SharedSocket>,
    #[cfg(unix)]
    link_options: HashMap<u64, Value>,
}

impl ReferencePeer {
    fn new(mutation: Mutation) -> Self {
        Self {
            peer_id: None,
            mutation,
            cells: BTreeMap::new(),
            seen: BTreeSet::new(),
            last_stamp: None,
            feature_observations: BTreeMap::new(),
            #[cfg(unix)]
            links: HashMap::new(),
            #[cfg(unix)]
            link_options: HashMap::new(),
        }
    }

    fn handle(&mut self, request: &Value) -> Value {
        let Some(cmd) = request.get("cmd").and_then(Value::as_str) else {
            return json!({"ok": false, "error": "missing cmd"});
        };
        match cmd {
            "hello" => self.hello(request),
            "local_set" => self.local_set(request),
            "deliver" => self.deliver(request),
            "snapshot" => self.snapshot(),
            "feature_reset" => self.feature_reset(request),
            "feature_step" => self.feature_step(request),
            "feature_observe" => self.feature_observe(request),
            "link_open" => self.link_open(request),
            "link_send" => self.link_send(request),
            "link_recv" => self.link_recv(request),
            "link_close" => self.link_close(request),
            "link_stats" => json!({
                "ok": true,
                "buffered_bytes": 0,
                "copies": 1,
                "reconnects": 0
            }),
            "bye" => json!({"ok": true}),
            _ => json!({"ok": false, "error": format!("unknown command {cmd}")}),
        }
    }

    fn hello(&mut self, request: &Value) -> Value {
        if request.get("protocol_version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
            return json!({
                "ok": false,
                "error": format!(
                    "unsupported protocol_version {}",
                    request.get("protocol_version").unwrap_or(&Value::Null)
                )
            });
        }
        let Some(peer_id) = request.get("peer").and_then(Value::as_u64) else {
            return json!({"ok": false, "error": "hello requires peer"});
        };
        self.peer_id = Some(peer_id);

        #[cfg(unix)]
        let channels = vec![
            "in_process_ffi",
            "shared_memory",
            "ipc_socket",
            "websocket",
            "webrtc_datachannel",
        ];
        #[cfg(not(unix))]
        let channels: Vec<&str> = Vec::new();
        #[cfg(unix)]
        let channel_variants = json!({
            "in_process_ffi": ["reference_c_abi_loopback"],
            "shared_memory": ["reference_mmap_control_loopback"],
            "ipc_socket": ["unix_stream_length_prefix"],
            "websocket": ["reference_ws_frame_loopback"],
            "webrtc_datachannel": ["reference_reliable_ordered_loopback"]
        });
        #[cfg(not(unix))]
        let channel_variants = json!({});
        let mut features = vec![
            "distributed_crdt".to_owned(),
            "transport_links".to_owned(),
            "durable_client_v1".to_owned(),
        ];
        if let Ok(extra) = std::env::var("LAZILY_INTEROP_EXTRA_FEATURES") {
            features.extend(
                extra
                    .split(',')
                    .map(str::trim)
                    .filter(|feature| !feature.is_empty())
                    .map(str::to_owned),
            );
        }

        json!({
            "ok": true,
            "binding": "lazily-interop-reference",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": PROTOCOL_VERSION,
            "features": features,
            "codecs": ["json"],
            "channels": channels,
            "channel_variants": channel_variants,
            "platform_profile": if cfg!(unix) { "unix" } else { "portable" },
            "carve_outs": []
        })
    }

    fn local_set(&mut self, request: &Value) -> Value {
        let Some(peer) = self.peer_id else {
            return json!({"ok": false, "error": "hello must run first"});
        };
        let Some(node) = request.get("node").and_then(Value::as_u64) else {
            return json!({"ok": false, "error": "local_set requires node"});
        };
        let key = match request.get("key") {
            Some(Value::String(key)) => Some(key.clone()),
            Some(Value::Null) => None,
            _ => return json!({"ok": false, "error": "local_set requires nullable key"}),
        };
        let Some(state) = request.get("state").cloned() else {
            return json!({"ok": false, "error": "local_set requires state"});
        };
        let Some(at) = request.get("at").and_then(Value::as_u64) else {
            return json!({"ok": false, "error": "local_set requires at"});
        };

        let logical = self
            .last_stamp
            .filter(|stamp| stamp.wall_time == at)
            .map_or(0, |stamp| stamp.logical + 1);
        let stamp = Stamp {
            wall_time: at,
            logical,
            peer,
        };
        self.last_stamp = Some(stamp);
        self.cells.insert(
            (node, key.clone()),
            Cell {
                stamp,
                state: state.clone(),
            },
        );

        let mut wire_state = state;
        if self.mutation == Mutation::TruncatedFrame
            && let Some(bytes) = wire_state.get_mut("Inline").and_then(Value::as_array_mut)
        {
            bytes.pop();
        }
        let mut op = Map::from_iter([
            ("node".to_owned(), json!(node)),
            ("key".to_owned(), json!(key)),
            ("stamp".to_owned(), json!(stamp)),
            ("state".to_owned(), wire_state),
        ]);
        if self.mutation == Mutation::OmitNullKey && op.get("key") == Some(&Value::Null) {
            op.remove("key");
        }

        let frontier = if self.mutation == Mutation::WrongFrontier {
            json!([{"peer": peer, "stamp": stamp}])
        } else {
            json!([[peer, stamp]])
        };
        let frame = json!({"CrdtSync": {"frontier": frontier, "ops": [op]}});
        json!({"ok": true, "frame": frame})
    }

    fn deliver(&mut self, request: &Value) -> Value {
        let Some(ops) = request
            .pointer("/frame/CrdtSync/ops")
            .and_then(Value::as_array)
        else {
            return json!({"ok": false, "error": "deliver requires CrdtSync ops"});
        };
        let mut applied = 0usize;
        for op in ops {
            let signature = match serde_json::to_string(op) {
                Ok(signature) => signature,
                Err(error) => return json!({"ok": false, "error": error.to_string()}),
            };
            if !self.seen.insert(signature) {
                continue;
            }
            applied += 1;
            let Some(node) = op.get("node").and_then(Value::as_u64) else {
                return json!({"ok": false, "error": "op requires node"});
            };
            let key = match op.get("key") {
                Some(Value::String(key)) => Some(key.clone()),
                Some(Value::Null) => None,
                _ => return json!({"ok": false, "error": "op requires nullable key"}),
            };
            let stamp: Stamp = match op.get("stamp").cloned().map(serde_json::from_value) {
                Some(Ok(stamp)) => stamp,
                _ => return json!({"ok": false, "error": "op requires stamp"}),
            };
            let Some(state) = op.get("state").cloned() else {
                return json!({"ok": false, "error": "op requires state"});
            };
            let current = self.cells.get(&(node, key.clone()));
            let wins = match current {
                None => true,
                Some(cell) if self.mutation == Mutation::ReverseStamp => stamp < cell.stamp,
                Some(cell) => stamp > cell.stamp,
            };
            if wins {
                self.cells.insert((node, key), Cell { stamp, state });
            }
        }
        if self.mutation == Mutation::FalseApplied && applied == 0 && !ops.is_empty() {
            applied = ops.len();
        }
        json!({"ok": true, "applied": applied})
    }

    fn snapshot(&self) -> Value {
        let cells = self
            .cells
            .iter()
            .map(|((node, key), cell)| {
                json!({
                    "node": node,
                    "key": key,
                    "state": cell.state
                })
            })
            .collect::<Vec<_>>();
        json!({"ok": true, "cells": cells})
    }

    fn feature_reset(&mut self, request: &Value) -> Value {
        let Some(feature) = request.get("feature").and_then(Value::as_str) else {
            return json!({"ok": false, "error": "feature_reset requires feature"});
        };
        self.feature_observations.remove(feature);
        json!({"ok": true, "feature": feature})
    }

    fn feature_step(&mut self, request: &Value) -> Value {
        let Some(feature) = request.get("feature").and_then(Value::as_str) else {
            return json!({"ok": false, "error": "feature_step requires feature"});
        };
        if feature != "durable_client_v1" {
            return json!({"ok": false, "error": format!("unsupported feature {feature}")});
        }
        let Some(step) = request.get("step") else {
            return json!({"ok": false, "error": "feature_step requires step"});
        };
        let observation = match durable_client_observation(step) {
            Ok(observation) => observation,
            Err(error) => return json!({"ok": false, "error": error}),
        };
        self.feature_observations
            .insert(feature.to_owned(), observation.clone());
        json!({"ok": true, "feature": feature, "observation": observation})
    }

    fn feature_observe(&self, request: &Value) -> Value {
        let Some(feature) = request.get("feature").and_then(Value::as_str) else {
            return json!({"ok": false, "error": "feature_observe requires feature"});
        };
        match self.feature_observations.get(feature) {
            Some(observation) => {
                json!({"ok": true, "feature": feature, "observation": observation})
            }
            None => json!({"ok": false, "error": "feature has no observation"}),
        }
    }

    fn link_open(&mut self, request: &Value) -> Value {
        let channel = request
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if ![
            "in_process_ffi",
            "shared_memory",
            "ipc_socket",
            "websocket",
            "webrtc_datachannel",
        ]
        .contains(&channel)
        {
            if self.mutation == Mutation::SwallowedUnsupported {
                return json!({"ok": true});
            }
            return json!({"ok": false, "error": "unsupported channel", "unsupported": true});
        }
        #[cfg(unix)]
        {
            match self.link_open_unix(request) {
                Ok(()) => json!({"ok": true, "codec": "json"}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            json!({"ok": false, "error": "unsupported channel", "unsupported": true})
        }
    }

    fn link_send(&mut self, request: &Value) -> Value {
        #[cfg(unix)]
        {
            match self.link_send_unix(request) {
                Ok(bytes) => json!({"ok": true, "bytes": bytes}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            json!({"ok": false, "error": "ipc_socket unavailable"})
        }
    }

    fn link_recv(&mut self, request: &Value) -> Value {
        #[cfg(unix)]
        {
            match self.link_recv_unix(request) {
                Ok((frames, bytes)) => json!({"ok": true, "frames": frames, "bytes": bytes}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            json!({"ok": false, "error": "ipc_socket unavailable"})
        }
    }

    fn link_close(&mut self, request: &Value) -> Value {
        #[cfg(unix)]
        if let Some(link) = request.get("link").and_then(Value::as_u64) {
            self.links.remove(&link);
            self.link_options.remove(&link);
        }
        json!({"ok": true})
    }

    #[cfg(unix)]
    fn link_open_unix(&mut self, request: &Value) -> Result<()> {
        let link = request
            .get("link")
            .and_then(Value::as_u64)
            .context("link_open requires link")?;
        let role = request
            .get("role")
            .and_then(Value::as_str)
            .context("link_open requires role")?;
        let endpoint = request
            .get("endpoint")
            .and_then(Value::as_str)
            .context("link_open requires endpoint")?;
        let shared = SharedSocket::default();
        match role {
            "listen" => {
                let listener = UnixListener::bind(endpoint)
                    .with_context(|| format!("bind ipc endpoint {endpoint}"))?;
                let accepted = shared.clone();
                thread::spawn(move || {
                    if let Ok((stream, _)) = listener.accept() {
                        accepted.install(stream);
                    }
                });
            }
            "connect" => {
                let deadline = Instant::now() + Duration::from_secs(2);
                let stream = loop {
                    match UnixStream::connect(endpoint) {
                        Ok(stream) => break stream,
                        Err(error)
                            if matches!(
                                error.kind(),
                                ErrorKind::NotFound | ErrorKind::ConnectionRefused
                            ) && Instant::now() < deadline =>
                        {
                            thread::yield_now();
                        }
                        Err(error) => {
                            return Err(error)
                                .with_context(|| format!("connect ipc endpoint {endpoint}"));
                        }
                    }
                };
                shared.install(stream);
            }
            _ => anyhow::bail!("invalid link role {role:?}"),
        }
        self.links.insert(link, shared);
        self.link_options.insert(
            link,
            request.get("options").cloned().unwrap_or_else(|| json!({})),
        );
        Ok(())
    }

    #[cfg(unix)]
    fn link_send_unix(&mut self, request: &Value) -> Result<usize> {
        let link = request
            .get("link")
            .and_then(Value::as_u64)
            .context("link_send requires link")?;
        let frame = request.get("frame").context("link_send requires frame")?;
        let mut payload = serde_json::to_vec(frame)?;
        let length = u32::try_from(payload.len()).context("frame exceeds u32 length prefix")?;
        let mut stream = self
            .links
            .get(&link)
            .context("unknown link")?
            .stream(Duration::from_secs(2))?;
        let fault = self
            .link_options
            .get(&link)
            .and_then(|options| options.get("fault"))
            .and_then(Value::as_str);
        let prefix = length.to_be_bytes();
        if fault == Some("split_prefix") {
            stream.write_all(&prefix[..1])?;
            stream.flush()?;
            thread::yield_now();
            stream.write_all(&prefix[1..])?;
        } else {
            stream.write_all(&prefix)?;
        }
        if self.mutation == Mutation::CloseAfterPrefix {
            stream.shutdown(Shutdown::Write)?;
            return Ok(4);
        }
        if self.mutation == Mutation::CorruptPayload && !payload.is_empty() {
            payload[0] = b'[';
        }
        if matches!(
            fault,
            Some("split_body" | "fragment" | "chunk" | "backpressure")
        ) && payload.len() > 1
        {
            for chunk in payload.chunks(payload.len().div_ceil(3)) {
                stream.write_all(chunk)?;
                stream.flush()?;
                thread::yield_now();
            }
        } else {
            stream.write_all(&payload)?;
        }
        stream.flush()?;
        Ok(4 + payload.len())
    }

    #[cfg(unix)]
    fn link_recv_unix(&mut self, request: &Value) -> Result<(Vec<Value>, usize)> {
        let link = request
            .get("link")
            .and_then(Value::as_u64)
            .context("link_recv requires link")?;
        let expect = request
            .get("expect")
            .and_then(Value::as_u64)
            .context("link_recv requires expect")? as usize;
        let timeout = Duration::from_millis(
            request
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(2_000),
        );
        let mut stream = self
            .links
            .get(&link)
            .context("unknown link")?
            .stream(timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        let mut frames = Vec::with_capacity(expect);
        let mut bytes = 0usize;
        for _ in 0..expect {
            let mut prefix = [0u8; 4];
            stream.read_exact(&mut prefix)?;
            let length = u32::from_be_bytes(prefix) as usize;
            let mut payload = vec![0u8; length];
            stream.read_exact(&mut payload)?;
            frames.push(serde_json::from_slice(&payload)?);
            bytes += 4 + length;
        }
        if self.mutation == Mutation::MergedFrames && frames.len() > 1 {
            frames = vec![json!({"Merged": frames})];
        }
        if self.mutation == Mutation::ReorderedFrames {
            frames.reverse();
        }
        Ok((frames, bytes))
    }
}

fn durable_client_observation(step: &Value) -> Result<Value, String> {
    let operation = step
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| "durable client step requires operation".to_owned())?;
    match operation {
        "validate_envelope" => {
            let envelope = step
                .get("envelope")
                .and_then(Value::as_object)
                .ok_or_else(|| "validate_envelope requires envelope".to_owned())?;
            let protocol = envelope
                .get("protocol_version")
                .and_then(Value::as_u64)
                .ok_or_else(|| "envelope requires protocol_version".to_owned())?;
            let (accepted, reason, payload_decoded) = if protocol != 1 {
                (false, "unsupported_protocol_version", false)
            } else if envelope
                .get("message_id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                (false, "invalid_message_id", false)
            } else if envelope.get("schema_version").and_then(Value::as_u64) == Some(0) {
                (false, "invalid_schema_version", false)
            } else if envelope.get("codec_version").and_then(Value::as_u64) == Some(0) {
                (false, "invalid_codec_version", false)
            } else {
                let payload = envelope
                    .get("payload")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "envelope requires byte payload".to_owned())?;
                if payload
                    .iter()
                    .any(|byte| byte.as_u64().is_none_or(|byte| byte > 255))
                {
                    return Err("envelope payload contains a non-byte".to_owned());
                }
                (true, "accepted", true)
            };
            Ok(json!({
                "accepted": accepted,
                "reason": reason,
                "payload_decoded": payload_decoded,
                "owner_authority": false
            }))
        }
        "order_projection" => {
            let observed = step
                .get("observed_source_positions")
                .and_then(Value::as_array)
                .ok_or_else(|| "order_projection requires positions".to_owned())?;
            let mut applied_through = 0_u64;
            let mut buffered = BTreeSet::new();
            let mut applied = Vec::new();
            let mut classifications = Vec::new();
            for value in observed {
                let position = value
                    .as_u64()
                    .ok_or_else(|| "source position must be unsigned".to_owned())?;
                if position <= applied_through || buffered.contains(&position) {
                    classifications.push("duplicate");
                } else if position > applied_through + 1 {
                    buffered.insert(position);
                    classifications.push("buffered");
                } else {
                    classifications.push("applied");
                    applied_through = position;
                    applied.push(position);
                    while buffered.remove(&(applied_through + 1)) {
                        applied_through += 1;
                        applied.push(applied_through);
                    }
                }
            }
            Ok(json!({
                "applied_source_positions": applied,
                "delivery_classification": classifications,
                "broker_order_authoritative": false,
                "may_authorize_transition": false
            }))
        }
        "classify_dedup" => {
            let deliveries = step
                .get("deliveries")
                .and_then(Value::as_array)
                .ok_or_else(|| "classify_dedup requires deliveries".to_owned())?;
            let mut first_by_id = BTreeMap::<String, Value>::new();
            let mut classification = Vec::new();
            for delivery in deliveries {
                let id = delivery
                    .get("message_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| "delivery requires message_id".to_owned())?;
                match first_by_id.get(id) {
                    None => {
                        first_by_id.insert(id.to_owned(), delivery.clone());
                        classification.push("first");
                    }
                    Some(first) if first == delivery => classification.push("duplicate"),
                    Some(_) => classification.push("conflict"),
                }
            }
            Ok(json!({"classification": classification, "owner_authority": false}))
        }
        "observe_receipt" => {
            let receipt = step
                .get("receipt")
                .and_then(Value::as_object)
                .ok_or_else(|| "observe_receipt requires receipt".to_owned())?;
            if receipt.get("protocol_version").and_then(Value::as_u64) != Some(1) {
                return Err("unsupported receipt protocol".to_owned());
            }
            for field in ["receipt_id", "message_id"] {
                if receipt
                    .get(field)
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    return Err(format!("receipt requires {field}"));
                }
            }
            let outcome = receipt
                .get("outcome")
                .and_then(Value::as_str)
                .ok_or_else(|| "receipt requires outcome".to_owned())?;
            if !["committed", "duplicate", "conflict", "rejected"].contains(&outcome) {
                return Err("invalid terminal receipt outcome".to_owned());
            }
            Ok(json!({
                "receipt": receipt,
                "terminal_owner_receipt": true,
                "transport_ack_equivalent": false,
                "owner_authority": false
            }))
        }
        "compare_projection_fingerprints" => {
            let left = step
                .get("left")
                .and_then(Value::as_object)
                .ok_or_else(|| "fingerprint comparison requires left".to_owned())?;
            let right = step
                .get("right")
                .and_then(Value::as_object)
                .ok_or_else(|| "fingerprint comparison requires right".to_owned())?;
            for observation in [left, right] {
                if observation
                    .get("may_authorize_transition")
                    .and_then(Value::as_bool)
                    != Some(false)
                {
                    return Err("projection observation claims transition authority".to_owned());
                }
                if !matches!(
                    observation.get("completeness").and_then(Value::as_str),
                    Some("complete_history" | "latest_state_only")
                ) {
                    return Err("invalid projection completeness".to_owned());
                }
            }
            let same_source = left.get("source_position") == right.get("source_position");
            let same_fingerprint = left.get("fingerprint") == right.get("fingerprint");
            let same_completeness = left.get("completeness") == right.get("completeness");
            let same_projection = left.get("projection_id") == right.get("projection_id");
            Ok(json!({
                "same_source": same_source,
                "same_fingerprint": same_fingerprint,
                "same_completeness": same_completeness,
                "equivalent": same_projection && same_source && same_fingerprint && same_completeness,
                "may_authorize_transition": false
            }))
        }
        other => Err(format!("unknown durable client operation {other}")),
    }
}

pub fn run_reference_peer() -> Result<()> {
    let mutation = Mutation::from_env();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let mut peer = ReferencePeer::new(mutation);

    for line in stdin.lock().lines() {
        let line = line.context("read control request")?;
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                writeln!(
                    output,
                    "{}",
                    json!({"ok": false, "error": error.to_string()})
                )?;
                output.flush()?;
                continue;
            }
        };
        let bye = request.get("cmd").and_then(Value::as_str) == Some("bye");
        let response = peer.handle(&request);
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        if mutation != Mutation::NonFlushed {
            output.flush()?;
        }
        if bye {
            break;
        }
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Clone, Default)]
struct SharedSocket {
    state: Arc<(Mutex<Option<UnixStream>>, Condvar)>,
}

#[cfg(unix)]
impl SharedSocket {
    fn install(&self, stream: UnixStream) {
        let (lock, ready) = &*self.state;
        if let Ok(mut target) = lock.lock() {
            *target = Some(stream);
            ready.notify_all();
        }
    }

    fn stream(&self, timeout: Duration) -> Result<UnixStream> {
        let (lock, ready) = &*self.state;
        let guard = lock
            .lock()
            .map_err(|_| anyhow::anyhow!("link lock poisoned"))?;
        let (guard, _) = ready
            .wait_timeout_while(guard, timeout, |stream| stream.is_none())
            .map_err(|_| anyhow::anyhow!("link lock poisoned"))?;
        guard
            .as_ref()
            .context("link accept timed out")?
            .try_clone()
            .context("clone unix stream")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_peer_wins_equal_stamp() {
        let mut peer = ReferencePeer::new(Mutation::None);
        peer.peer_id = Some(1);
        peer.local_set(&json!({
            "node": 1,
            "key": null,
            "state": {"Inline": [1]},
            "at": 10
        }));
        let result = peer.deliver(&json!({
            "frame": {
                "CrdtSync": {
                    "ops": [{
                        "node": 1,
                        "key": null,
                        "stamp": {"wall_time": 10, "logical": 0, "peer": 2},
                        "state": {"Inline": [2]}
                    }]
                }
            }
        }));
        assert_eq!(result["applied"], 1);
        assert_eq!(peer.snapshot()["cells"][0]["state"]["Inline"][0], 2);
    }

    #[test]
    fn duplicate_delivery_applies_zero() {
        let mut peer = ReferencePeer::new(Mutation::None);
        let request = json!({
            "frame": {
                "CrdtSync": {
                    "ops": [{
                        "node": 1,
                        "key": null,
                        "stamp": {"wall_time": 10, "logical": 0, "peer": 2},
                        "state": {"Inline": [2]}
                    }]
                }
            }
        });
        assert_eq!(peer.deliver(&request)["applied"], 1);
        assert_eq!(peer.deliver(&request)["applied"], 0);
    }
}
