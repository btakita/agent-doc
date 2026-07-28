use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    Semantic,
    Transport,
    Harness,
}

impl fmt::Display for Plane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Semantic => f.write_str("semantic"),
            Self::Transport => f.write_str("transport"),
            Self::Harness => f.write_str("harness"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    PeerLaunch,
    PeerTimeout,
    PeerProtocol,
    UnsupportedContract,
    UnknownCapability,
    MissingRequiredCodec,
    MissingRequiredChannel,
    InsufficientEligiblePeers,
    SchemaViolation,
    Convergence,
    DuplicateApplied,
    FrameCount,
    FrameTruncated,
    UnsupportedSwallowed,
    ProcessExit,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "peer_protocol".to_owned());
        f.write_str(&text)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Failure {
    pub category: ErrorCategory,
    pub plane: Plane,
    pub culprit: String,
    #[serde(flatten)]
    pub diagnostics: Box<FailureDiagnostics>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FailureDiagnostics {
    pub channel: Option<String>,
    pub codec: Option<String>,
    pub first_divergence: Option<String>,
    pub peer_stderr: Option<String>,
    pub exit_status: Option<i32>,
}

impl Failure {
    pub fn new(
        category: ErrorCategory,
        plane: Plane,
        culprit: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            category,
            plane,
            culprit: culprit.into(),
            diagnostics: Box::default(),
            message: message.into(),
        }
    }

    pub fn with_channel_codec(
        mut self,
        channel: impl Into<String>,
        codec: impl Into<String>,
    ) -> Self {
        self.diagnostics.channel = Some(channel.into());
        self.diagnostics.codec = Some(codec.into());
        self
    }

    pub fn with_first_divergence(mut self, divergence: impl Into<String>) -> Self {
        self.diagnostics.first_divergence = Some(divergence.into());
        self
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} [{}] {}",
            self.plane, self.category, self.culprit, self.message
        )
    }
}

impl std::error::Error for Failure {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Coverage {
    pub peers_started: usize,
    pub frames_emitted: usize,
    pub frames_delivered: usize,
    pub feature_steps_run: usize,
    pub channels_run: usize,
    pub transport_cells: Vec<TransportCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportCell {
    pub sender: String,
    pub receiver: String,
    pub direction: String,
    pub profile: String,
    pub plane: Plane,
    pub channel: String,
    pub variant: String,
    pub codec: String,
    pub frame_family: String,
    pub scenario: String,
    pub seed: u64,
    pub status: RunStatus,
    pub unsupported_reason: Option<String>,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub first_divergence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub protocol_version: u64,
    pub scenario: String,
    pub seed: u64,
    pub status: RunStatus,
    pub plane: Plane,
    pub peers: Vec<String>,
    pub staged_peers: Vec<StagedPeer>,
    pub coverage: Coverage,
    pub failures: Vec<Failure>,
    pub replay: String,
}

impl RunReport {
    pub fn pass(
        scenario: impl Into<String>,
        seed: u64,
        peers: Vec<String>,
        coverage: Coverage,
        replay: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION,
            scenario: scenario.into(),
            seed,
            status: RunStatus::Pass,
            plane: Plane::Semantic,
            peers,
            staged_peers: Vec::new(),
            coverage,
            failures: Vec::new(),
            replay: replay.into(),
        }
    }

    pub fn fail(
        scenario: impl Into<String>,
        seed: u64,
        peers: Vec<String>,
        coverage: Coverage,
        replay: impl Into<String>,
        failure: Failure,
    ) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION,
            scenario: scenario.into(),
            seed,
            status: RunStatus::Fail,
            plane: failure.plane,
            peers,
            staged_peers: Vec::new(),
            coverage,
            failures: vec![failure],
            replay: replay.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    pub peers: BTreeMap<String, PeerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PeerConfig {
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub seed: u64,
    pub peers: Vec<ScenarioPeer>,
    pub required_features: Vec<String>,
    pub required_codecs: Vec<String>,
    pub required_channels: Vec<String>,
    #[serde(default)]
    pub frame_sets: BTreeMap<String, Vec<Value>>,
    pub steps: Vec<ScenarioStep>,
    pub expected_snapshot: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPeer {
    pub name: String,
    pub peer_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ScenarioStep {
    LocalSet {
        peer: String,
        node: u64,
        key: Option<String>,
        state: Value,
        at: u64,
        save_as: String,
    },
    Deliver {
        frame: String,
        to: Vec<String>,
        at: u64,
        expect_applied: usize,
    },
    Transport {
        from: String,
        to: String,
        channel: String,
        variant: String,
        codec: String,
        frame_set: String,
        #[serde(default)]
        fault: Option<String>,
        #[serde(default)]
        bidirectional: bool,
    },
    TransportMatrix {
        channel: String,
        codec: String,
        frame_set: String,
        #[serde(default)]
        fault: Option<String>,
    },
    FeatureReset {
        feature: String,
    },
    FeatureStep {
        feature: String,
        step: Value,
        expected: Value,
    },
    FeatureObserve {
        feature: String,
        expected: Value,
    },
    AssertConverged,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloReply {
    pub ok: bool,
    pub binding: String,
    pub version: String,
    pub protocol_version: u64,
    pub features: Vec<String>,
    pub codecs: Vec<String>,
    pub channels: Vec<String>,
    #[serde(default)]
    pub channel_variants: BTreeMap<String, Vec<String>>,
    pub platform_profile: String,
    #[serde(default)]
    pub carve_outs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PeerCapabilities {
    pub binding: String,
    pub version: String,
    pub features: Vec<String>,
    pub codecs: Vec<String>,
    pub channels: Vec<String>,
    pub channel_variants: BTreeMap<String, Vec<String>>,
    pub platform_profile: String,
    pub carve_outs: Vec<String>,
}

impl From<HelloReply> for PeerCapabilities {
    fn from(reply: HelloReply) -> Self {
        Self {
            binding: reply.binding,
            version: reply.version,
            features: reply.features,
            codecs: reply.codecs,
            channels: reply.channels,
            channel_variants: reply.channel_variants,
            platform_profile: reply.platform_profile,
            carve_outs: reply.carve_outs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagedPeer {
    pub peer: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationProof {
    pub mutation: String,
    pub expected_category: ErrorCategory,
    pub observed_category: ErrorCategory,
    pub culprit: String,
    pub caught: bool,
}
