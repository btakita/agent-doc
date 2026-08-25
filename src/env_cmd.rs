//! `agent-doc env` captures terminal-host facts once and delegates classification.

use agent_doc_config::terminal_host::{ResolvedTerminalHost, TerminalHostReport, classify};
use std::collections::BTreeMap;
use std::process::{Command, Stdio};

pub fn run(json: bool) -> anyhow::Result<()> {
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let tmux_installed = command_succeeds("tmux", &["-V"]);
    let tmux_server_running = tmux_installed && command_succeeds("tmux", &["has-session"]);
    let report = classify(&env, tmux_installed, tmux_server_running);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }
    Ok(())
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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
