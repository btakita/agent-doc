use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result, Worktree};

/// Tells `agent-doc zed-lsp` which extension generation launched it; the
/// server reports it as `editor_version` in its replica registration.
const EXTENSION_VERSION_ENV: &str = "AGENT_DOC_ZED_EXTENSION_VERSION";

/// The version Zed installed this extension under: `extension.toml`, not the
/// WASM crate's `Cargo.toml`.
fn extension_version() -> &'static str {
    include_str!("../extension.toml")
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "version").then(|| value.trim().trim_matches('"'))
        })
        .unwrap_or("unknown")
}

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
            env: vec![(
                EXTENSION_VERSION_ENV.to_string(),
                extension_version().to_string(),
            )],
        })
    }
}

zed::register_extension!(AgentDocExtension);
