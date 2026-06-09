//! Stable-prefix prompt assembly helpers for provider prompt caches.
//!
//! The provider decides what to cache, but agent-doc controls prompt ordering.
//! Keep durable instructions above [`PROMPT_CACHE_BOUNDARY`] and move
//! turn-specific data such as diffs, queue heads, status, and compaction
//! diagnostics below it.

pub const PROMPT_CACHE_BOUNDARY: &str =
    "<agent_doc_prompt_cache_boundary volatile_suffix=\"follows\" />";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCacheBlocks {
    stable_prefix: String,
    volatile_suffix: String,
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
}
