use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result, Worktree};

struct AgentDocExtension;

impl zed::Extension for AgentDocExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<zed::Command> {
        let configured = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let command = configured
            .as_ref()
            .and_then(|binary| binary.path.clone())
            .or_else(|| worktree.which("agent-doc"))
            .ok_or_else(|| {
                "agent-doc was not found in the Zed worktree PATH; install agent-doc or configure lsp.agent-doc.binary.path"
                    .to_string()
            })?;
        let args = configured
            .and_then(|binary| binary.arguments)
            .unwrap_or_else(|| vec!["zed-lsp".to_string()]);
        Ok(zed::Command {
            command,
            args,
            env: Vec::new(),
        })
    }
}

zed::register_extension!(AgentDocExtension);
