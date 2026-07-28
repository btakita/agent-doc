use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::PROTOCOL_VERSION;
use crate::schema::Schemas;
use crate::types::{ErrorCategory, Failure, HelloReply, PeerCapabilities, PeerConfig, Plane};

const CODECS: &[&str] = &["json", "msgpack", "postcard"];
const FEATURES: &[&str] = &[
    "distributed_crdt",
    "reliable_sync",
    "causal_receipts",
    "signaling",
    "transport_links",
    "stdlib_timer_v1",
    "stdlib_timeout_v1",
    "stdlib_revision_barrier_v1",
];
const CHANNELS: &[&str] = &[
    "in_process_ffi",
    "shared_memory",
    "ipc_socket",
    "websocket",
    "webrtc_datachannel",
];
const PLATFORM_PROFILES: &[&str] = &["portable", "unix", "windows", "wasm"];

pub struct PeerProcess {
    name: String,
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Result<String, String>>,
    stderr: Arc<Mutex<String>>,
    timeout: Duration,
    capabilities: PeerCapabilities,
}

impl PeerProcess {
    pub fn start(
        name: &str,
        config: &PeerConfig,
        peer_id: u64,
        timeout: Duration,
        schemas: &Schemas,
    ) -> Result<Self, Failure> {
        let (program, args) = config.command.split_first().ok_or_else(|| {
            Failure::new(
                ErrorCategory::PeerLaunch,
                Plane::Harness,
                name,
                "peer command is empty",
            )
        })?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        command.envs(&config.env);

        let mut child = command.spawn().map_err(|error| {
            Failure::new(
                ErrorCategory::PeerLaunch,
                Plane::Harness,
                name,
                format!("could not launch {:?}: {error}", config.command),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            Failure::new(
                ErrorCategory::PeerLaunch,
                Plane::Harness,
                name,
                "child stdin was not piped",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            Failure::new(
                ErrorCategory::PeerLaunch,
                Plane::Harness,
                name,
                "child stdout was not piped",
            )
        })?;
        let stderr_pipe = child.stderr.take().ok_or_else(|| {
            Failure::new(
                ErrorCategory::PeerLaunch,
                Plane::Harness,
                name,
                "child stderr was not piped",
            )
        })?;

        let (reply_tx, replies) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        while line.ends_with(['\n', '\r']) {
                            line.pop();
                        }
                        if reply_tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = reply_tx.send(Err(error.to_string()));
                        break;
                    }
                }
            }
        });

        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_capture = Arc::clone(&stderr);
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr_pipe);
            let mut text = String::new();
            let _ = reader.read_to_string(&mut text);
            if let Ok(mut target) = stderr_capture.lock() {
                *target = text;
            }
        });

        let mut peer = Self {
            name: name.to_owned(),
            child,
            stdin,
            replies,
            stderr,
            timeout,
            capabilities: PeerCapabilities {
                binding: String::new(),
                version: String::new(),
                features: Vec::new(),
                codecs: Vec::new(),
                channels: Vec::new(),
                channel_variants: Default::default(),
                platform_profile: String::new(),
                carve_outs: Vec::new(),
            },
        };

        let value = peer.exchange(
            &json!({
                "cmd": "hello",
                "peer": peer_id,
                "protocol_version": PROTOCOL_VERSION
            }),
            schemas,
        )?;
        if value.get("ok") != Some(&Value::Bool(true)) {
            return Err(Failure::new(
                ErrorCategory::UnsupportedContract,
                Plane::Harness,
                name,
                format!(
                    "peer rejected control protocol v{PROTOCOL_VERSION}: {}",
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("no error supplied")
                ),
            ));
        }
        let hello: HelloReply = serde_json::from_value(value).map_err(|error| {
            Failure::new(
                ErrorCategory::PeerProtocol,
                Plane::Harness,
                name,
                format!("malformed hello response: {error}"),
            )
        })?;
        if !hello.ok || hello.protocol_version != PROTOCOL_VERSION {
            return Err(Failure::new(
                ErrorCategory::UnsupportedContract,
                Plane::Harness,
                name,
                format!(
                    "hello selected protocol v{}, expected v{PROTOCOL_VERSION}",
                    hello.protocol_version
                ),
            ));
        }
        validate_capabilities(name, &hello)?;
        peer.capabilities = hello.into();
        Ok(peer)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn capabilities(&self) -> &PeerCapabilities {
        &self.capabilities
    }

    pub fn exchange(&mut self, request: &Value, schemas: &Schemas) -> Result<Value, Failure> {
        let encoded = serde_json::to_string(request).map_err(|error| {
            Failure::new(
                ErrorCategory::PeerProtocol,
                Plane::Harness,
                &self.name,
                format!("could not encode control request: {error}"),
            )
        })?;
        writeln!(self.stdin, "{encoded}")
            .and_then(|()| self.stdin.flush())
            .map_err(|error| {
                Failure::new(
                    ErrorCategory::ProcessExit,
                    Plane::Harness,
                    &self.name,
                    format!(
                        "could not write control request: {error}{}",
                        self.stderr_suffix()
                    ),
                )
            })?;

        let line = match self.replies.recv_timeout(self.timeout) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(Failure::new(
                    ErrorCategory::ProcessExit,
                    Plane::Harness,
                    &self.name,
                    format!(
                        "could not read control response: {error}{}",
                        self.stderr_suffix()
                    ),
                ));
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                return Err(Failure::new(
                    ErrorCategory::PeerTimeout,
                    Plane::Harness,
                    &self.name,
                    format!(
                        "no flushed response to {:?} within {} ms{}",
                        request.get("cmd").and_then(Value::as_str),
                        self.timeout.as_millis(),
                        self.stderr_suffix()
                    ),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                let status = self.child.try_wait().ok().flatten();
                return Err(Failure::new(
                    ErrorCategory::ProcessExit,
                    Plane::Harness,
                    &self.name,
                    format!(
                        "peer closed stdout while answering {:?} (status {status:?}){}",
                        request.get("cmd").and_then(Value::as_str),
                        self.stderr_suffix()
                    ),
                ));
            }
        };
        let value: Value = serde_json::from_str(&line).map_err(|error| {
            Failure::new(
                ErrorCategory::PeerProtocol,
                Plane::Harness,
                &self.name,
                format!("non-JSON control response {line:?}: {error}"),
            )
        })?;
        schemas.validate_control(&value, &self.name)?;
        Ok(value)
    }

    pub fn request_ok(&mut self, request: &Value, schemas: &Schemas) -> Result<Value, Failure> {
        let value = self.exchange(request, schemas)?;
        if value.get("ok") == Some(&Value::Bool(true)) {
            Ok(value)
        } else {
            Err(Failure::new(
                ErrorCategory::PeerProtocol,
                Plane::Harness,
                &self.name,
                format!(
                    "peer rejected {:?}: {}",
                    request.get("cmd").and_then(Value::as_str),
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("no error supplied")
                ),
            ))
        }
    }

    pub fn close(mut self, schemas: &Schemas) -> Result<(), Failure> {
        let response = self.request_ok(&json!({"cmd": "bye"}), schemas);
        response?;
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => {
                    return Err(Failure::new(
                        ErrorCategory::ProcessExit,
                        Plane::Harness,
                        &self.name,
                        format!("peer exited with {status}{}", self.stderr_suffix()),
                    ));
                }
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Err(Failure::new(
                        ErrorCategory::PeerTimeout,
                        Plane::Harness,
                        &self.name,
                        format!(
                            "peer acknowledged bye but did not exit within {} ms",
                            self.timeout.as_millis()
                        ),
                    ));
                }
                Err(error) => {
                    return Err(Failure::new(
                        ErrorCategory::ProcessExit,
                        Plane::Harness,
                        &self.name,
                        format!("could not wait for peer exit: {error}"),
                    ));
                }
            }
        }
    }

    fn stderr_suffix(&self) -> String {
        self.stderr
            .lock()
            .ok()
            .filter(|text| !text.trim().is_empty())
            .map(|text| format!("\n--- {} stderr ---\n{}", self.name, text.trim_end()))
            .unwrap_or_default()
    }
}

impl Drop for PeerProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn validate_transport_receive(
    culprit: &str,
    expected_frames: &[Value],
    reply: &Value,
) -> Result<(), Failure> {
    let frames = reply
        .get("frames")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Failure::new(
                ErrorCategory::PeerProtocol,
                Plane::Transport,
                culprit,
                "link_recv response has no frames array",
            )
        })?;
    if frames.len() != expected_frames.len() {
        return Err(Failure::new(
            ErrorCategory::FrameCount,
            Plane::Transport,
            culprit,
            format!(
                "received {} whole frames, expected {}",
                frames.len(),
                expected_frames.len()
            ),
        ));
    }
    for (index, (actual, expected)) in frames.iter().zip(expected_frames).enumerate() {
        if actual != expected {
            return Err(Failure::new(
                ErrorCategory::FrameTruncated,
                Plane::Transport,
                culprit,
                format!("frame {index} differs after channel decode"),
            ));
        }
    }
    Ok(())
}

fn validate_capabilities(name: &str, hello: &HelloReply) -> Result<(), Failure> {
    for codec in &hello.codecs {
        if !CODECS.contains(&codec.as_str()) {
            return Err(unknown_capability(name, "codec", codec));
        }
    }
    for feature in &hello.features {
        if !FEATURES.contains(&feature.as_str()) {
            return Err(unknown_capability(name, "feature", feature));
        }
    }
    for channel in &hello.channels {
        if !CHANNELS.contains(&channel.as_str()) {
            return Err(unknown_capability(name, "channel", channel));
        }
    }
    for channel in hello.channel_variants.keys() {
        if !CHANNELS.contains(&channel.as_str()) || !hello.channels.contains(channel) {
            return Err(unknown_capability(name, "channel variant owner", channel));
        }
    }
    if !PLATFORM_PROFILES.contains(&hello.platform_profile.as_str()) {
        return Err(unknown_capability(
            name,
            "platform profile",
            &hello.platform_profile,
        ));
    }

    let codecs = hello.codecs.iter().collect::<BTreeSet<_>>();
    if codecs.len() != hello.codecs.len() {
        return Err(unknown_capability(name, "duplicate codec", "duplicate"));
    }
    let features = hello.features.iter().collect::<BTreeSet<_>>();
    if features.len() != hello.features.len() {
        return Err(unknown_capability(name, "duplicate feature", "duplicate"));
    }
    let channels = hello.channels.iter().collect::<BTreeSet<_>>();
    if channels.len() != hello.channels.len() {
        return Err(unknown_capability(name, "duplicate channel", "duplicate"));
    }
    Ok(())
}

fn unknown_capability(name: &str, kind: &str, value: &str) -> Failure {
    Failure::new(
        ErrorCategory::UnknownCapability,
        Plane::Harness,
        name,
        format!("unknown {kind} {value:?}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_frames_fail_closed() {
        let frames = vec![json!({"a": 1}), json!({"b": 2})];
        let reply = json!({"ok": true, "frames": [{"Merged": frames}]});
        let error = validate_transport_receive("receiver", &frames, &reply).unwrap_err();
        assert_eq!(error.category, ErrorCategory::FrameCount);
        assert_eq!(error.culprit, "receiver");
    }

    #[test]
    fn unknown_codec_is_attributed() {
        let hello = HelloReply {
            ok: true,
            binding: "bad".into(),
            version: "1".into(),
            protocol_version: 1,
            features: Vec::new(),
            codecs: vec!["yaml".into()],
            channels: Vec::new(),
            channel_variants: Default::default(),
            platform_profile: "portable".into(),
            carve_outs: Vec::new(),
        };
        let error = validate_capabilities("bad", &hello).unwrap_err();
        assert_eq!(error.category, ErrorCategory::UnknownCapability);
        assert_eq!(error.culprit, "bad");
    }
}
