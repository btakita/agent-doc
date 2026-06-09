//! Stable-prefix prompt assembly helpers for provider prompt caches.
//!
//! The provider decides what to cache, but agent-doc controls prompt ordering.
//! Keep durable instructions above [`PROMPT_CACHE_BOUNDARY`] and move
//! turn-specific data such as diffs, queue heads, status, and compaction
//! diagnostics below it.
//!
//! Boundary contract:
//! - stable prefix: response contract, harness-neutral behavior instructions,
//!   and provider cache metadata that should replay across turns.
//! - volatile suffix: file paths, queue heads, diffs, current document excerpts,
//!   status, prompt targets, session-accretion/context packs, and any other
//!   turn-local facts.
//! - provider key: version + routing-affinity hash + stable-prefix hash. The
//!   volatile suffix never contributes to the replay key.

use sha2::{Digest, Sha256};

pub const PROMPT_CACHE_BOUNDARY: &str =
    "<agent_doc_prompt_cache_boundary cache_control=\"ephemeral\" volatile_suffix=\"follows\" />";
pub const PROMPT_CACHE_CONTROL: &str = r#"{"type":"ephemeral"}"#;
const PROVIDER_CACHE_KEY_VERSION: &str = "agent-doc-prompt-cache-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheBlocks {
    stable_prefix: String,
    volatile_suffix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheReplayKey {
    pub stable_prefix_sha256: String,
    pub provider_cache_key: String,
    pub cache_control: String,
    pub routing_affinity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheSessionCostSample {
    pub stable_prefix_sha256: String,
    pub adapter_state: String,
    pub routing_affinity: String,
    pub cached_input_tokens: Option<u64>,
    pub creation_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheMissCause {
    pub cause: &'static str,
    pub impact: u64,
    pub detail: String,
}

impl PromptCacheBlocks {
    pub fn new(
        stable_prefix: impl Into<String>,
        volatile_suffix: impl Into<String>,
    ) -> PromptCacheBlocks {
        PromptCacheBlocks {
            stable_prefix: stable_prefix.into(),
            volatile_suffix: volatile_suffix.into(),
        }
    }

    pub fn render(&self) -> String {
        render_prompt_cache_blocks(&self.stable_prefix, &self.volatile_suffix)
    }

    pub fn from_rendered(rendered: &str) -> Option<PromptCacheBlocks> {
        let (stable_prefix, volatile_suffix) = rendered.split_once(PROMPT_CACHE_BOUNDARY)?;
        Some(PromptCacheBlocks::new(
            stable_prefix.trim_end(),
            volatile_suffix.trim_start(),
        ))
    }

    pub fn stable_prefix(&self) -> &str {
        self.stable_prefix.trim_end()
    }

    pub fn replay_key(&self, routing_affinity: impl AsRef<str>) -> PromptCacheReplayKey {
        let stable_prefix_sha256 = content_sha256(self.stable_prefix());
        let routing_affinity = routing_affinity.as_ref().trim().to_string();
        let routing_sha256 = content_sha256(&routing_affinity);
        PromptCacheReplayKey {
            provider_cache_key: format!(
                "{PROVIDER_CACHE_KEY_VERSION}:{routing_sha256}:{stable_prefix_sha256}"
            ),
            stable_prefix_sha256,
            cache_control: PROMPT_CACHE_CONTROL.to_string(),
            routing_affinity,
        }
    }
}

impl PromptCacheSessionCostSample {
    pub fn from_replay_key(
        replay_key: &PromptCacheReplayKey,
        adapter_state: impl Into<String>,
    ) -> Self {
        PromptCacheSessionCostSample {
            stable_prefix_sha256: replay_key.stable_prefix_sha256.clone(),
            adapter_state: adapter_state.into(),
            routing_affinity: replay_key.routing_affinity.clone(),
            cached_input_tokens: None,
            creation_tokens: None,
        }
    }

    pub fn with_usage(
        mut self,
        cached_input_tokens: Option<u64>,
        creation_tokens: Option<u64>,
    ) -> Self {
        self.cached_input_tokens = cached_input_tokens;
        self.creation_tokens = creation_tokens;
        self
    }
}

pub fn render_prompt_cache_blocks(stable_prefix: &str, volatile_suffix: &str) -> String {
    let stable = stable_prefix.trim_end();
    let volatile = volatile_suffix.trim_start();
    let mut rendered =
        String::with_capacity(stable.len() + PROMPT_CACHE_BOUNDARY.len() + volatile.len() + 4);
    rendered.push_str(stable);
    rendered.push_str("\n\n");
    rendered.push_str(PROMPT_CACHE_BOUNDARY);
    rendered.push_str("\n\n");
    rendered.push_str(volatile);
    rendered
}

pub fn rank_cache_miss_causes(
    previous: &PromptCacheSessionCostSample,
    current: &PromptCacheSessionCostSample,
) -> Vec<PromptCacheMissCause> {
    let cached_loss = cached_input_loss(previous, current).unwrap_or(0);
    let creation_spike = creation_token_spike(previous, current).unwrap_or(0);
    let token_pressure = cached_loss.saturating_add(creation_spike).max(1);
    let mut causes = Vec::new();

    if previous.stable_prefix_sha256 != current.stable_prefix_sha256 {
        causes.push(PromptCacheMissCause {
            cause: "fingerprint",
            impact: 5_000_000u64.saturating_add(token_pressure),
            detail: format!(
                "stable_prefix_sha256 changed {} -> {}",
                previous.stable_prefix_sha256, current.stable_prefix_sha256
            ),
        });
    }

    if previous.adapter_state != current.adapter_state {
        causes.push(PromptCacheMissCause {
            cause: "adapter_state",
            impact: 4_000_000u64.saturating_add(token_pressure),
            detail: format!(
                "adapter_state changed {} -> {}",
                previous.adapter_state, current.adapter_state
            ),
        });
    }

    if previous.routing_affinity != current.routing_affinity {
        causes.push(PromptCacheMissCause {
            cause: "routing_affinity",
            impact: 3_000_000u64.saturating_add(token_pressure),
            detail: format!(
                "routing_affinity changed {} -> {}",
                previous.routing_affinity, current.routing_affinity
            ),
        });
    }

    if cached_loss > 0 {
        causes.push(PromptCacheMissCause {
            cause: "cached_input_delta",
            impact: cached_loss,
            detail: format!(
                "cached_input_tokens dropped by {} ({} -> {})",
                cached_loss,
                previous.cached_input_tokens.unwrap_or_default(),
                current.cached_input_tokens.unwrap_or_default()
            ),
        });
    }

    if creation_spike > 0 {
        causes.push(PromptCacheMissCause {
            cause: "creation_token_spike",
            impact: creation_spike,
            detail: format!(
                "creation_tokens rose by {} ({} -> {})",
                creation_spike,
                previous.creation_tokens.unwrap_or_default(),
                current.creation_tokens.unwrap_or_default()
            ),
        });
    }

    causes.sort_by(|left, right| {
        right
            .impact
            .cmp(&left.impact)
            .then_with(|| left.cause.cmp(right.cause))
    });
    causes
}

pub fn render_cache_miss_ranking(
    previous: Option<&PromptCacheSessionCostSample>,
    current: &PromptCacheSessionCostSample,
) -> String {
    let cached_delta = previous
        .map(|prev| signed_token_delta(prev.cached_input_tokens, current.cached_input_tokens))
        .unwrap_or_else(|| "unknown".to_string());
    let creation_spike = previous
        .map(|prev| signed_positive_delta(prev.creation_tokens, current.creation_tokens))
        .unwrap_or_else(|| "unknown".to_string());
    let miss_rank = match previous {
        Some(prev) => {
            let ranked = rank_cache_miss_causes(prev, current);
            if ranked.is_empty() {
                "none".to_string()
            } else {
                ranked
                    .iter()
                    .enumerate()
                    .map(|(index, cause)| {
                        format!("{}:{}(impact={})", index + 1, cause.cause, cause.impact)
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            }
        }
        None => "baseline_required".to_string(),
    };

    format!(
        "fingerprint={} adapter_state={} routing_affinity={} cached_input_delta={} creation_token_spike={} miss_rank={}",
        current.stable_prefix_sha256,
        current.adapter_state,
        current.routing_affinity,
        cached_delta,
        creation_spike,
        miss_rank
    )
}

fn cached_input_loss(
    previous: &PromptCacheSessionCostSample,
    current: &PromptCacheSessionCostSample,
) -> Option<u64> {
    let previous = previous.cached_input_tokens?;
    let current = current.cached_input_tokens?;
    Some(previous.saturating_sub(current))
}

fn creation_token_spike(
    previous: &PromptCacheSessionCostSample,
    current: &PromptCacheSessionCostSample,
) -> Option<u64> {
    let previous = previous.creation_tokens?;
    let current = current.creation_tokens?;
    Some(current.saturating_sub(previous))
}

fn signed_token_delta(previous: Option<u64>, current: Option<u64>) -> String {
    match (previous, current) {
        (Some(previous), Some(current)) if current >= previous => {
            format!("+{}", current - previous)
        }
        (Some(previous), Some(current)) => format!("-{}", previous - current),
        _ => "unknown".to_string(),
    }
}

fn signed_positive_delta(previous: Option<u64>, current: Option<u64>) -> String {
    match (previous, current) {
        (Some(previous), Some(current)) if current > previous => format!("+{}", current - previous),
        (Some(_), Some(_)) => "0".to_string(),
        _ => "unknown".to_string(),
    }
}

fn content_sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_prompt_cache_blocks_places_volatile_suffix_after_boundary() {
        let rendered =
            PromptCacheBlocks::new("stable instructions", "volatile queue head").render();
        let boundary = rendered.find(PROMPT_CACHE_BOUNDARY).unwrap();
        let volatile = rendered.find("volatile queue head").unwrap();

        assert!(rendered.starts_with("stable instructions"));
        assert!(volatile > boundary);
    }

    #[test]
    fn replay_key_tracks_stable_prefix_route_and_provider_cache_control() {
        let blocks = PromptCacheBlocks::new("stable instructions", "volatile queue head");
        let first = blocks.replay_key("agent=codex;model=gpt-5;mode=template");
        let second = blocks.replay_key("agent=codex;model=gpt-5;mode=template");
        let changed_route = blocks.replay_key("agent=claude;model=opus;mode=template");
        let changed_stable =
            PromptCacheBlocks::new("stable instructions\nDurable instruction", "volatile")
                .replay_key("agent=codex;model=gpt-5;mode=template");

        assert_eq!(first, second);
        assert_eq!(first.cache_control, PROMPT_CACHE_CONTROL);
        assert_ne!(first.provider_cache_key, changed_route.provider_cache_key);
        assert_eq!(
            first.stable_prefix_sha256,
            changed_route.stable_prefix_sha256
        );
        assert_ne!(
            first.stable_prefix_sha256,
            changed_stable.stable_prefix_sha256
        );
        assert_ne!(first.provider_cache_key, changed_stable.provider_cache_key);
    }

    #[test]
    fn boundary_contract_exposes_provider_breakpoint_and_key_material() {
        assert!(PROMPT_CACHE_BOUNDARY.contains("cache_control=\"ephemeral\""));
        assert!(PROMPT_CACHE_BOUNDARY.contains("volatile_suffix=\"follows\""));

        let blocks = PromptCacheBlocks::new("stable instructions", "volatile queue head");
        let rendered = blocks.render();
        assert_eq!(rendered.matches(PROMPT_CACHE_BOUNDARY).count(), 1);

        let replay_key = blocks.replay_key("agent=codex;model=gpt-5;mode=template");
        let key_parts: Vec<&str> = replay_key.provider_cache_key.split(':').collect();
        assert_eq!(key_parts.len(), 3);
        assert_eq!(key_parts[0], PROVIDER_CACHE_KEY_VERSION);
        assert_eq!(replay_key.cache_control, PROMPT_CACHE_CONTROL);
        assert_eq!(
            replay_key.stable_prefix_sha256,
            content_sha256(blocks.stable_prefix())
        );
    }

    #[test]
    fn cache_miss_ranking_orders_fingerprint_before_usage_symptoms() {
        let previous = PromptCacheSessionCostSample {
            stable_prefix_sha256: "aaa".to_string(),
            adapter_state: "resume:sess-1".to_string(),
            routing_affinity: "agent=codex;model=gpt-5;mode=template".to_string(),
            cached_input_tokens: Some(120_000),
            creation_tokens: Some(1_000),
        };
        let current = PromptCacheSessionCostSample {
            stable_prefix_sha256: "bbb".to_string(),
            adapter_state: previous.adapter_state.clone(),
            routing_affinity: previous.routing_affinity.clone(),
            cached_input_tokens: Some(2_000),
            creation_tokens: Some(90_000),
        };

        let ranked = rank_cache_miss_causes(&previous, &current);

        assert_eq!(ranked[0].cause, "fingerprint");
        assert!(
            ranked
                .iter()
                .any(|cause| cause.cause == "cached_input_delta")
        );
        assert!(
            ranked
                .iter()
                .any(|cause| cause.cause == "creation_token_spike")
        );
        assert_eq!(ranked[0].impact, 5_207_000);
    }

    #[test]
    fn cache_miss_ranking_surfaces_adapter_and_route_ahead_of_token_symptoms() {
        let previous = PromptCacheSessionCostSample {
            stable_prefix_sha256: "stable".to_string(),
            adapter_state: "resume:sess-1".to_string(),
            routing_affinity: "agent=codex;model=gpt-5;mode=template".to_string(),
            cached_input_tokens: Some(80_000),
            creation_tokens: Some(500),
        };
        let current = PromptCacheSessionCostSample {
            stable_prefix_sha256: previous.stable_prefix_sha256.clone(),
            adapter_state: "fresh".to_string(),
            routing_affinity: "agent=codex;model=gpt-5.1;mode=template".to_string(),
            cached_input_tokens: Some(10_000),
            creation_tokens: Some(12_000),
        };

        let ranked = rank_cache_miss_causes(&previous, &current);
        let names: Vec<&str> = ranked.iter().map(|cause| cause.cause).collect();

        assert_eq!(
            names,
            vec![
                "adapter_state",
                "routing_affinity",
                "cached_input_delta",
                "creation_token_spike"
            ]
        );
    }

    #[test]
    fn cache_miss_ranking_renderer_is_operator_readable() {
        let previous = PromptCacheSessionCostSample {
            stable_prefix_sha256: "aaa".to_string(),
            adapter_state: "resume:sess-1".to_string(),
            routing_affinity: "agent=codex;model=gpt-5;mode=template".to_string(),
            cached_input_tokens: Some(120_000),
            creation_tokens: Some(1_000),
        };
        let current = PromptCacheSessionCostSample {
            stable_prefix_sha256: "bbb".to_string(),
            adapter_state: "fresh".to_string(),
            routing_affinity: previous.routing_affinity.clone(),
            cached_input_tokens: Some(2_000),
            creation_tokens: Some(90_000),
        };

        let rendered = render_cache_miss_ranking(Some(&previous), &current);

        assert!(rendered.contains("fingerprint=bbb"));
        assert!(rendered.contains("adapter_state=fresh"));
        assert!(rendered.contains("routing_affinity=agent=codex;model=gpt-5;mode=template"));
        assert!(rendered.contains("cached_input_delta=-118000"));
        assert!(rendered.contains("creation_token_spike=+89000"));
        assert!(rendered.contains("miss_rank=1:fingerprint"));
    }

    #[test]
    fn cache_miss_ranking_without_baseline_requests_history() {
        let key = PromptCacheBlocks::new("stable instructions", "volatile")
            .replay_key("agent=codex;model=gpt-5;mode=template");
        let current = PromptCacheSessionCostSample::from_replay_key(&key, "fresh");

        let rendered = render_cache_miss_ranking(None, &current);

        assert!(rendered.contains("cached_input_delta=unknown"));
        assert!(rendered.contains("creation_token_spike=unknown"));
        assert!(rendered.contains("miss_rank=baseline_required"));
    }
}
