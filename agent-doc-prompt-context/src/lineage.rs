//! Bounded, serializable provenance carried by prompts produced during a turn.

use serde::{Deserialize, Serialize};

pub const PROMPT_CONTEXT_VERSION: u16 = 1;
pub const PROMPT_CONTEXT_OPEN: &str = "<agent-doc-prompt-context>";
pub const PROMPT_CONTEXT_CLOSE: &str = "</agent-doc-prompt-context>";

const MAX_IDENTITY_CHARS: usize = 256;
const MAX_ORIGIN_CHARS: usize = 2_048;
const MAX_CONTEXT_CHARS: usize = 1_024;
const MAX_EVIDENCE: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptContextPacket {
    pub version: u16,
    pub origin: PromptOrigin,
    pub response: Option<ResponseLineage>,
    pub selection: Option<PromptSelection>,
    pub decision_evidence: Vec<String>,
    pub producer: PromptProducer,
    pub claim: Option<ClaimLineage>,
}

impl Default for PromptContextPacket {
    fn default() -> Self {
        Self {
            version: PROMPT_CONTEXT_VERSION,
            origin: PromptOrigin::default(),
            response: None,
            selection: None,
            decision_evidence: Vec::new(),
            producer: PromptProducer::default(),
            claim: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptOrigin {
    pub item_id: Option<String>,
    pub item_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResponseLineage {
    pub response_id: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptSelection {
    pub kind: PromptSelectionKind,
    pub label: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSelectionKind {
    Choice,
    Action,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptProducer {
    pub kind: PromptProducerKind,
    pub identity: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptProducerKind {
    Queue,
    Operator,
    SubagentReturn,
    SubagentFollowUp,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaimLineage {
    pub generation: Option<u64>,
    pub worker: Option<String>,
}

impl PromptContextPacket {
    /// Apply the contract's size limits before serialization or transport.
    pub fn bounded(mut self) -> Self {
        self.version = PROMPT_CONTEXT_VERSION;
        self.origin.item_id = bound_option(self.origin.item_id, MAX_IDENTITY_CHARS);
        self.origin.item_text = bound(self.origin.item_text, MAX_ORIGIN_CHARS);
        if let Some(response) = self.response.as_mut() {
            response.response_id = bound_option(response.response_id.take(), MAX_IDENTITY_CHARS);
            response.summary = bound(std::mem::take(&mut response.summary), MAX_CONTEXT_CHARS);
        }
        if let Some(selection) = self.selection.as_mut() {
            selection.label = bound_option(selection.label.take(), MAX_IDENTITY_CHARS);
            selection.text = bound(std::mem::take(&mut selection.text), MAX_CONTEXT_CHARS);
        }
        self.decision_evidence.truncate(MAX_EVIDENCE);
        for evidence in &mut self.decision_evidence {
            *evidence = bound(std::mem::take(evidence), MAX_CONTEXT_CHARS);
        }
        self.producer.identity = bound_option(self.producer.identity.take(), MAX_IDENTITY_CHARS);
        if let Some(claim) = self.claim.as_mut() {
            claim.worker = bound_option(claim.worker.take(), MAX_IDENTITY_CHARS);
        }
        self
    }
}

/// Stable serialization used by every prompt producer.
pub fn serialize(packet: &PromptContextPacket) -> String {
    let packet = packet.clone().bounded();
    let json = serde_json::to_string(&packet).expect("prompt context schema is serializable");
    format!("{PROMPT_CONTEXT_OPEN}\n{json}\n{PROMPT_CONTEXT_CLOSE}")
}

/// Add exactly one packet to a prompt, replacing any packet already attached.
pub fn attach(packet: &PromptContextPacket, prompt: &str) -> String {
    let (_, prompt) = split(prompt);
    format!("{}\n\n{}", serialize(packet), prompt.trim_start())
}

/// Split a leading packet from a prompt. Invalid or non-leading packets are inert.
pub fn split(prompt: &str) -> (Option<PromptContextPacket>, &str) {
    let trimmed = prompt.trim_start();
    let Some(after_open) = trimmed.strip_prefix(PROMPT_CONTEXT_OPEN) else {
        return (None, prompt);
    };
    let Some((json, after_close)) = after_open
        .strip_prefix('\n')
        .and_then(|body| body.split_once(&format!("\n{PROMPT_CONTEXT_CLOSE}")))
    else {
        return (None, prompt);
    };
    let Ok(packet) = serde_json::from_str::<PromptContextPacket>(json) else {
        return (None, prompt);
    };
    (
        Some(packet.bounded()),
        after_close.trim_start_matches(['\r', '\n']),
    )
}

fn bound_option(value: Option<String>, limit: usize) -> Option<String> {
    value
        .map(|value| bound(value, limit))
        .filter(|value| !value.is_empty())
}

fn bound(value: String, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value;
    }
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_round_trips_and_replaces_existing_packet() {
        let packet = PromptContextPacket {
            origin: PromptOrigin {
                item_id: Some("q-7".into()),
                item_text: "ship it".into(),
            },
            response: Some(ResponseLineage {
                response_id: Some("[#r-a]".into()),
                summary: "ready".into(),
            }),
            producer: PromptProducer {
                kind: PromptProducerKind::Operator,
                identity: Some("operator".into()),
            },
            ..PromptContextPacket::default()
        };
        let once = attach(&packet, "do it");
        let twice = attach(&packet, &once);
        assert_eq!(once, twice);
        assert_eq!(split(&once), (Some(packet), "do it"));
        assert_eq!(once.matches(PROMPT_CONTEXT_OPEN).count(), 1);
    }

    #[test]
    fn old_json_gets_backward_safe_defaults() {
        let prompt = format!("{PROMPT_CONTEXT_OPEN}\n{{}}\n{PROMPT_CONTEXT_CLOSE}\n\nwork");
        let (packet, body) = split(&prompt);
        assert_eq!(packet, Some(PromptContextPacket::default()));
        assert_eq!(body, "work");
    }

    #[test]
    fn bounds_untrusted_context_without_splitting_unicode() {
        let packet = PromptContextPacket {
            origin: PromptOrigin {
                item_text: "🦀".repeat(MAX_ORIGIN_CHARS + 1),
                ..PromptOrigin::default()
            },
            decision_evidence: (0..MAX_EVIDENCE + 2)
                .map(|_| "é".repeat(MAX_CONTEXT_CHARS + 1))
                .collect(),
            ..PromptContextPacket::default()
        }
        .bounded();
        assert_eq!(packet.origin.item_text.chars().count(), MAX_ORIGIN_CHARS);
        assert_eq!(packet.decision_evidence.len(), MAX_EVIDENCE);
        assert!(
            packet
                .decision_evidence
                .iter()
                .all(|value| value.chars().count() == MAX_CONTEXT_CHARS)
        );
    }
}
