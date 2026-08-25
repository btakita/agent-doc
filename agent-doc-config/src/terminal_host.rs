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
}
