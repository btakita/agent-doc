pub mod claude;

use anyhow::Result;

/// Response from an agent backend.
pub struct AgentResponse {
    pub text: String,
    pub session_id: Option<String>,
}

/// Agent backend trait — send a prompt, get a response.
pub trait Agent {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse>;
}

/// Resolve an agent backend by name.
pub fn resolve(name: &str) -> Result<Box<dyn Agent>> {
    match name {
        "claude" => Ok(Box::new(claude::Claude)),
        other => anyhow::bail!("Unknown agent backend: {}", other),
    }
}
