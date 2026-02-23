pub mod claude;

use anyhow::Result;

use crate::config::AgentConfig;

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
pub fn resolve(name: &str, config: Option<&AgentConfig>) -> Result<Box<dyn Agent>> {
    let (cmd, args) = match config {
        Some(ac) => (Some(ac.command.clone()), Some(ac.args.clone())),
        None => (None, None),
    };
    match name {
        "claude" => Ok(Box::new(claude::Claude::new(cmd, args))),
        other => {
            if config.is_some() {
                Ok(Box::new(claude::Claude::new(cmd, args)))
            } else {
                anyhow::bail!("Unknown agent backend: {}", other)
            }
        }
    }
}
