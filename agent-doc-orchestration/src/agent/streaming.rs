//! Streaming agent adapter trait.
//!
//! Pure stream chunk vocabulary and JSON/JSONL line parsers live in
//! `agent_doc_turn_executor::agent_stream`.

use agent_doc_turn_executor::agent_stream::StreamChunk;
use anyhow::Result;

/// Trait for agent backends that support streaming output.
pub trait StreamingAgent {
    /// Send a prompt and return an iterator over response chunks.
    fn send_streaming(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamChunk>>>>;
}
