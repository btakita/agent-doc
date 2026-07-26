use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, BufRead, BufWriter, Write};

#[cfg(unix)]
use std::io::{ErrorKind, Read};
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
    #[cfg(unix)]
    links: HashMap<u64, SharedSocket>,
}

impl ReferencePeer {
    fn new(mutation: Mutation) -> Self {
        Self {
            peer_id: None,
            mutation,
            cells: BTreeMap::new(),
            seen: BTreeSet::new(),
            last_stamp: None,
            #[cfg(unix)]
            links: HashMap::new(),
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
        let channels = vec!["ipc_socket"];
        #[cfg(not(unix))]
        let channels: Vec<&str> = Vec::new();
        #[cfg(unix)]
        let channel_variants = json!({"ipc_socket": ["unix_stream_length_prefix"]});
        #[cfg(not(unix))]
        let channel_variants = json!({});

        json!({
            "ok": true,
            "binding": "lazily-interop-reference",
            "version": env!("CARGO_PKG_VERSION"),
            "protocol_version": PROTOCOL_VERSION,
            "features": ["distributed_crdt", "transport_links"],
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

    fn link_open(&mut self, request: &Value) -> Value {
        let channel = request
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if channel != "ipc_socket" {
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
        Ok(())
    }

    #[cfg(unix)]
    fn link_send_unix(&mut self, request: &Value) -> Result<usize> {
        let link = request
            .get("link")
            .and_then(Value::as_u64)
            .context("link_send requires link")?;
        let frame = request.get("frame").context("link_send requires frame")?;
        let payload = serde_json::to_vec(frame)?;
        let length = u32::try_from(payload.len()).context("frame exceeds u32 length prefix")?;
        let mut stream = self
            .links
            .get(&link)
            .context("unknown link")?
            .stream(Duration::from_secs(2))?;
        stream.write_all(&length.to_be_bytes())?;
        stream.write_all(&payload)?;
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
        Ok((frames, bytes))
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
