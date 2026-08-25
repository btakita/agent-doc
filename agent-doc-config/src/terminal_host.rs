//! Terminal-host capability classification shared by the CLI and editor plugins.
//!
//! The classifier is deliberately pure: the binary captures environment and tmux
//! process facts once, then passes those facts here. Editor integrations may add
//! authoritative IDE observations through the two `AGENT_DOC_*` bridge variables:
//!
//! - JetBrains: set `AGENT_DOC_JETBRAINS_PRODUCT_MODE=backend` from
//!   `IdeProductMode.isBackend` (the supported platform API; do not inspect the
//!   historical `idea.is.remote.dev` implementation property).
//! - VS Code: set `AGENT_DOC_VSCODE_REMOTE_NAME` to `vscode.env.remoteName` when
//!   it is defined. Remote-name values are extension-defined, so they stay opaque.
//!
//! Coder workspace detection follows the environment installed by the Coder
//! agent for workspace commands: `CODER=true` plus workspace identity variables.
//! `CODER_AGENT_TOKEN` is intentionally neither read nor serialized.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use agent_doc_frontmatter::{
    frontmatter::TerminalHostPreference, project_config::ProjectTerminalConfig,
};

use crate::TerminalConfig;

pub const JETBRAINS_PRODUCT_MODE_ENV: &str = "AGENT_DOC_JETBRAINS_PRODUCT_MODE";
pub const VSCODE_REMOTE_NAME_ENV: &str = "AGENT_DOC_VSCODE_REMOTE_NAME";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ide", rename_all = "snake_case")]
pub enum IdeRemoteKind {
    JetBrainsBackend,
    VsCode { remote_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalHostEnv {
    pub coder: bool,
    pub ide_remote: Option<IdeRemoteKind>,
    pub display: bool,
    pub ssh: bool,
    pub tmux_installed: bool,
    pub tmux_server_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedTerminalHost {
    Ide,
    External,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalHostReport {
    pub classification: TerminalHostEnv,
    pub resolved_terminal_host: ResolvedTerminalHost,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSessionState {
    Attached,
    Detached,
    Missing,
}

/// Fully resolved terminal policy. Editor adapters consume this through the
/// `tmux ensure --json` receipt instead of reimplementing config precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTerminalPolicy {
    pub configured_host: TerminalHostPreference,
    pub host: ResolvedTerminalHost,
    pub auto_start_tmux: bool,
    pub command: Option<String>,
    pub attach_command: Option<String>,
    pub reason: String,
    pub failure: Option<String>,
}

/// Resolve document → project → global terminal policy without filesystem or
/// process access. `ide_available` is supplied by an editor-originated CLI call;
/// `external_available` means an external command or shell host is usable.
pub fn resolve_terminal_policy(
    frontmatter_host: Option<TerminalHostPreference>,
    project: Option<&ProjectTerminalConfig>,
    global: Option<&TerminalConfig>,
    detected_host: ResolvedTerminalHost,
    ide_available: bool,
    external_available: bool,
    session_state: TerminalSessionState,
) -> ResolvedTerminalPolicy {
    let configured_host = frontmatter_host
        .or_else(|| project.and_then(|config| config.host))
        .or_else(|| global.and_then(|config| config.host))
        .unwrap_or_default();
    let auto_start_tmux = project
        .and_then(|config| config.auto_start_tmux)
        .or_else(|| global.and_then(|config| config.auto_start_tmux))
        .unwrap_or(true);
    let command = project
        .and_then(|config| config.command.clone())
        .or_else(|| global.and_then(|config| config.command.clone()));
    let attach_command = project
        .and_then(|config| config.attach_command.clone())
        .or_else(|| global.and_then(|config| config.attach_command.clone()));

    let (host, reason, failure) = if session_state == TerminalSessionState::Attached {
        (
            ResolvedTerminalHost::None,
            "the live project session already has an attached client".to_string(),
            None,
        )
    } else {
        match configured_host {
            TerminalHostPreference::Auto
                if ide_available || detected_host == ResolvedTerminalHost::Ide =>
            {
                (
                    ResolvedTerminalHost::Ide,
                    "auto selected the available IDE terminal host".to_string(),
                    None,
                )
            }
            TerminalHostPreference::Auto
                if external_available || detected_host == ResolvedTerminalHost::External =>
            {
                (
                    ResolvedTerminalHost::External,
                    "auto selected the available external terminal host".to_string(),
                    None,
                )
            }
            TerminalHostPreference::Auto => (
                ResolvedTerminalHost::None,
                "auto found no IDE endpoint or external terminal host".to_string(),
                None,
            ),
            TerminalHostPreference::Ide
                if ide_available || detected_host == ResolvedTerminalHost::Ide =>
            {
                (
                    ResolvedTerminalHost::Ide,
                    "terminal_host=ide selected the available IDE terminal host".to_string(),
                    None,
                )
            }
            TerminalHostPreference::Ide => {
                let message =
                    "terminal_host=ide was requested, but no IDE terminal endpoint is available";
                (
                    ResolvedTerminalHost::None,
                    message.to_string(),
                    Some(message.to_string()),
                )
            }
            TerminalHostPreference::External
                if external_available || detected_host == ResolvedTerminalHost::External =>
            {
                (
                    ResolvedTerminalHost::External,
                    "terminal_host=external selected the available external terminal host"
                        .to_string(),
                    None,
                )
            }
            TerminalHostPreference::External => {
                let message = "terminal_host=external was requested, but no external terminal command or shell host is available";
                (
                    ResolvedTerminalHost::None,
                    message.to_string(),
                    Some(message.to_string()),
                )
            }
            TerminalHostPreference::None => (
                ResolvedTerminalHost::None,
                "terminal_host=none disables terminal presentation".to_string(),
                None,
            ),
        }
    };

    ResolvedTerminalPolicy {
        configured_host,
        host,
        auto_start_tmux,
        command,
        attach_command,
        reason,
        failure,
    }
}

/// Combine the detected shell/display host with command availability. A command
/// string alone cannot make a headless Coder backend an external presentation
/// host; Coder needs either a real display/shell signal or an IDE endpoint.
pub fn external_host_available(report: &TerminalHostReport, command_available: bool) -> bool {
    report.resolved_terminal_host == ResolvedTerminalHost::External
        || (command_available && !report.classification.coder)
}

/// Classify an already-captured process environment and tmux probe.
pub fn classify(
    env: &BTreeMap<String, String>,
    tmux_installed: bool,
    tmux_server_running: bool,
) -> TerminalHostReport {
    let coder = truthy(env.get("CODER"))
        || any_nonempty(
            env,
            &[
                "CODER_WORKSPACE_NAME",
                "CODER_WORKSPACE_ID",
                "CODER_WORKSPACE_AGENT_NAME",
                "CODER_WORKSPACE_OWNER_NAME",
            ],
        );
    let ide_remote = ide_remote_kind(env);
    let display = any_nonempty(env, &["DISPLAY", "WAYLAND_DISPLAY"]);
    let ssh = any_nonempty(env, &["SSH_CONNECTION"]);

    let classification = TerminalHostEnv {
        coder,
        ide_remote,
        display,
        ssh,
        tmux_installed,
        tmux_server_running,
    };
    let (resolved_terminal_host, reason) = resolve(&classification);

    TerminalHostReport {
        classification,
        resolved_terminal_host,
        reason: reason.to_string(),
    }
}

fn ide_remote_kind(env: &BTreeMap<String, String>) -> Option<IdeRemoteKind> {
    if env
        .get(JETBRAINS_PRODUCT_MODE_ENV)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("backend"))
    {
        return Some(IdeRemoteKind::JetBrainsBackend);
    }

    env.get(VSCODE_REMOTE_NAME_ENV)
        .filter(|name| !name.trim().is_empty())
        .map(|remote_name| IdeRemoteKind::VsCode {
            remote_name: remote_name.clone(),
        })
}

fn resolve(env: &TerminalHostEnv) -> (ResolvedTerminalHost, &'static str) {
    if !env.tmux_installed {
        return (
            ResolvedTerminalHost::None,
            "tmux is not installed; add it to the host or workspace image",
        );
    }
    if env.ide_remote.is_some() {
        return (
            ResolvedTerminalHost::Ide,
            "an editor supplied an authoritative remote-host observation",
        );
    }
    if env.display {
        return (
            ResolvedTerminalHost::External,
            "DISPLAY or WAYLAND_DISPLAY is available for an external terminal",
        );
    }
    if env.ssh {
        return (
            ResolvedTerminalHost::External,
            "SSH_CONNECTION identifies the current external shell host",
        );
    }
    if env.coder {
        return (
            ResolvedTerminalHost::None,
            "Coder workspace detected, but no IDE remote-host observation is registered",
        );
    }
    (
        ResolvedTerminalHost::None,
        "headless environment has no IDE remote-host observation or external shell signal",
    )
}

fn any_nonempty(env: &BTreeMap<String, String>, names: &[&str]) -> bool {
    names
        .iter()
        .any(|name| env.get(*name).is_some_and(|value| !value.trim().is_empty()))
}

fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn laptop_display_resolves_external() {
        let report = classify(&env(&[("DISPLAY", ":0")]), true, true);

        assert!(report.classification.display);
        assert_eq!(
            report.resolved_terminal_host,
            ResolvedTerminalHost::External
        );
        assert!(report.reason.contains("DISPLAY"));
    }

    #[test]
    fn coder_container_preserves_vscode_remote_name_and_resolves_ide() {
        let report = classify(
            &env(&[
                ("CODER", "true"),
                ("CODER_WORKSPACE_ID", "workspace-id"),
                (VSCODE_REMOTE_NAME_ENV, "ssh-remote"),
            ]),
            true,
            false,
        );

        assert!(report.classification.coder);
        assert_eq!(
            report.classification.ide_remote,
            Some(IdeRemoteKind::VsCode {
                remote_name: "ssh-remote".to_string()
            })
        );
        assert_eq!(report.resolved_terminal_host, ResolvedTerminalHost::Ide);
    }

    #[test]
    fn plain_ssh_box_resolves_external() {
        let report = classify(
            &env(&[("SSH_CONNECTION", "192.0.2.1 1000 192.0.2.2 22")]),
            true,
            false,
        );

        assert!(report.classification.ssh);
        assert_eq!(
            report.resolved_terminal_host,
            ResolvedTerminalHost::External
        );
        assert!(report.reason.contains("SSH_CONNECTION"));
    }

    #[test]
    fn container_without_tmux_fails_closed_with_image_guidance() {
        let report = classify(
            &env(&[("CODER", "true"), (JETBRAINS_PRODUCT_MODE_ENV, "backend")]),
            false,
            false,
        );

        assert_eq!(report.resolved_terminal_host, ResolvedTerminalHost::None);
        assert!(report.reason.contains("workspace image"));
    }

    #[test]
    fn coder_token_alone_is_not_a_workspace_identity_signal() {
        let report = classify(&env(&[("CODER_AGENT_TOKEN", "secret")]), true, false);

        assert!(!report.classification.coder);
    }

    fn project_terminal_config(
        host: Option<TerminalHostPreference>,
        auto_start_tmux: Option<bool>,
        command: Option<&str>,
        attach_command: Option<&str>,
    ) -> ProjectTerminalConfig {
        ProjectTerminalConfig {
            host,
            auto_start_tmux,
            command: command.map(str::to_string),
            attach_command: attach_command.map(str::to_string),
        }
    }

    fn global_terminal_config(
        host: Option<TerminalHostPreference>,
        auto_start_tmux: Option<bool>,
        command: Option<&str>,
        attach_command: Option<&str>,
    ) -> TerminalConfig {
        TerminalConfig {
            host,
            auto_start_tmux,
            command: command.map(str::to_string),
            attach_command: attach_command.map(str::to_string),
        }
    }

    #[test]
    fn terminal_policy_precedence_is_frontmatter_then_project_then_global() {
        let project = project_terminal_config(
            Some(TerminalHostPreference::Ide),
            Some(false),
            Some("project-terminal {tmux_command}"),
            None,
        );
        let global = global_terminal_config(
            Some(TerminalHostPreference::External),
            Some(true),
            Some("global-terminal {tmux_command}"),
            Some("global-attach {session}"),
        );

        let resolved = resolve_terminal_policy(
            Some(TerminalHostPreference::None),
            Some(&project),
            Some(&global),
            ResolvedTerminalHost::External,
            true,
            true,
            TerminalSessionState::Missing,
        );

        assert_eq!(resolved.configured_host, TerminalHostPreference::None);
        assert_eq!(resolved.host, ResolvedTerminalHost::None);
        assert!(!resolved.auto_start_tmux);
        assert_eq!(
            resolved.command.as_deref(),
            Some("project-terminal {tmux_command}")
        );
        assert_eq!(
            resolved.attach_command.as_deref(),
            Some("global-attach {session}")
        );
    }

    #[test]
    fn terminal_policy_project_host_beats_global_and_auto_prefers_ide() {
        let project = project_terminal_config(Some(TerminalHostPreference::Ide), None, None, None);
        let global =
            global_terminal_config(Some(TerminalHostPreference::External), None, None, None);
        let explicit_project = resolve_terminal_policy(
            None,
            Some(&project),
            Some(&global),
            ResolvedTerminalHost::External,
            true,
            true,
            TerminalSessionState::Detached,
        );
        assert_eq!(explicit_project.host, ResolvedTerminalHost::Ide);

        let auto = resolve_terminal_policy(
            None,
            None,
            None,
            ResolvedTerminalHost::External,
            true,
            true,
            TerminalSessionState::Missing,
        );
        assert_eq!(auto.host, ResolvedTerminalHost::Ide);
    }

    #[test]
    fn terminal_policy_global_fallback_and_attached_session_noop() {
        let global = global_terminal_config(
            Some(TerminalHostPreference::External),
            Some(false),
            Some("wezterm {tmux_command}"),
            None,
        );
        let resolved = resolve_terminal_policy(
            None,
            None,
            Some(&global),
            ResolvedTerminalHost::None,
            false,
            true,
            TerminalSessionState::Detached,
        );
        assert_eq!(resolved.host, ResolvedTerminalHost::External);
        assert!(!resolved.auto_start_tmux);

        let attached = resolve_terminal_policy(
            Some(TerminalHostPreference::Ide),
            None,
            Some(&global),
            ResolvedTerminalHost::Ide,
            true,
            true,
            TerminalSessionState::Attached,
        );
        assert_eq!(attached.host, ResolvedTerminalHost::None);
        assert!(attached.reason.contains("already has an attached client"));
    }

    #[test]
    fn explicit_external_in_headless_coder_fails_closed() {
        let resolved = resolve_terminal_policy(
            Some(TerminalHostPreference::External),
            None,
            None,
            ResolvedTerminalHost::None,
            true,
            false,
            TerminalSessionState::Missing,
        );

        assert_eq!(resolved.host, ResolvedTerminalHost::None);
        assert!(resolved.failure.as_deref().is_some_and(|message| {
            message.contains("no external terminal command or shell host")
        }));
    }

    #[test]
    fn configured_command_does_not_make_headless_coder_external() {
        let report = classify(
            &env(&[("CODER", "true"), ("CODER_WORKSPACE_ID", "workspace-id")]),
            true,
            false,
        );

        assert!(!external_host_available(&report, true));
    }
}
