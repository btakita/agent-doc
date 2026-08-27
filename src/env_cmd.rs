//! `agent-doc env` captures terminal-host facts once and delegates classification.

use agent_doc_config::terminal_host::{ResolvedTerminalHost, TerminalHostReport, classify};
use serde::Serialize;
use std::collections::BTreeMap;
use std::process::Stdio;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TmuxProbe {
    pub(crate) binary: String,
    pub(crate) version: Option<String>,
    pub(crate) version_error: Option<String>,
    pub(crate) server_handshake: bool,
    pub(crate) server_version: Option<String>,
    pub(crate) server_error: Option<String>,
}

fn configured_tmux() -> tmux_router::Tmux {
    agent_doc_project_config_io::project_tmux_bin()
        .map(tmux_router::Tmux::default_server_with_binary)
        .unwrap_or_else(tmux_router::Tmux::default_server)
}

fn output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        stdout
    }
}

pub(crate) fn probe_tmux() -> TmuxProbe {
    probe_tmux_with(&configured_tmux())
}

fn probe_tmux_with(tmux: &tmux_router::Tmux) -> TmuxProbe {
    let binary = tmux.binary_path().display().to_string();
    let version_output = tmux.cmd().arg("-V").stdin(Stdio::null()).output();
    let (version, version_error) = match version_output {
        Ok(output) if output.status.success() => (Some(output_text(&output)), None),
        Ok(output) => (
            None,
            Some(format!(
                "{} -V exited {:?}: {}",
                binary,
                output.status.code(),
                output_text(&output)
            )),
        ),
        Err(error) => (None, Some(format!("failed to spawn {binary}: {error}"))),
    };

    let server_output = tmux
        .cmd()
        .args(["display-message", "-p", "#{version}"])
        .stdin(Stdio::null())
        .output();
    let (server_handshake, server_version, server_error) = match server_output {
        Ok(output) if output.status.success() => (true, Some(output_text(&output)), None),
        Ok(output) => (
            false,
            None,
            Some(format!(
                "{} server handshake exited {:?}: {}",
                binary,
                output.status.code(),
                output_text(&output)
            )),
        ),
        Err(error) => (
            false,
            None,
            Some(format!(
                "failed to spawn {binary} for server handshake: {error}"
            )),
        ),
    };

    TmuxProbe {
        binary,
        version,
        version_error,
        server_handshake,
        server_version,
        server_error,
    }
}

pub fn run(json: bool) -> anyhow::Result<()> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let tmux = probe_tmux();
    let tmux_installed = tmux.version.is_some();
    let tmux_server_running = tmux.server_handshake;
    let report = classify(&env, tmux_installed, tmux_server_running);

    if json {
        let mut value = serde_json::to_value(&report)?;
        value
            .as_object_mut()
            .expect("terminal host report serializes as an object")
            .insert("tmux_probe".to_string(), serde_json::to_value(&tmux)?);
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print_human(&report);
        print_tmux_probe(&tmux);
    }
    Ok(())
}

fn print_tmux_probe(probe: &TmuxProbe) {
    println!("tmux binary: {}", probe.binary);
    println!(
        "tmux version: {}",
        probe.version.as_deref().unwrap_or("unavailable")
    );
    println!("tmux server handshake: {}", probe.server_handshake);
    if let Some(version) = probe.server_version.as_deref() {
        println!("tmux server version: {version}");
    }
    if let Some(error) = probe.version_error.as_deref() {
        println!("tmux version error: {error}");
    }
    if let Some(error) = probe.server_error.as_deref() {
        println!("tmux server error: {error}");
    }
}

fn print_human(report: &TerminalHostReport) {
    let host = match report.resolved_terminal_host {
        ResolvedTerminalHost::Ide => "ide",
        ResolvedTerminalHost::External => "external",
        ResolvedTerminalHost::None => "none",
    };
    println!("terminal host: {host}");
    println!("reason: {}", report.reason);
    println!("coder: {}", report.classification.coder);
    println!("ide remote: {:?}", report.classification.ide_remote);
    println!("display: {}", report.classification.display);
    println!("ssh: {}", report.classification.ssh);
    println!("tmux installed: {}", report.classification.tmux_installed);
    println!(
        "tmux server running: {}",
        report.classification.tmux_server_running
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_the_exact_unavailable_binary() {
        let tmux = tmux_router::Tmux::default_server_with_binary("/missing/tmux-client");
        let probe = probe_tmux_with(&tmux);
        assert_eq!(probe.binary, "/missing/tmux-client");
        assert!(probe.version.is_none());
        assert!(
            probe
                .version_error
                .as_deref()
                .is_some_and(|error| error.contains("/missing/tmux-client"))
        );
        assert!(!probe.server_handshake);
    }
}
