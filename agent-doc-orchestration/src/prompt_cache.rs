//! Stable-prefix prompt assembly helpers for provider prompt caches.
//!
//! The provider decides what to cache, but agent-doc controls prompt ordering.
//! Keep durable instructions above [`PROMPT_CACHE_BOUNDARY`] and move
//! turn-specific data such as diffs, queue heads, status, and compaction
//! diagnostics below it.

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
}
