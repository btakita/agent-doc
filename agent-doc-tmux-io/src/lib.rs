//! Effectful tmux command execution adapter.
//!
//! This crate owns subprocess effects for tmux commands. It does not own
//! document authority, merge behavior, queue projection, or turn commits.

pub mod observation_cache;

pub use observation_cache::{
    ObservationScopeStats, TmuxObservationScope, begin_observation_scope,
    observation_scope_stats,
};

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Output};

use agent_doc_tmux_commands::{
    TmuxCommand, TmuxSubmitProfile, capture_pane as capture_pane_command,
    capture_pane_with_ansi as capture_pane_with_ansi_command, display_message,
    display_notification, kill_pane as kill_pane_command, kill_window as kill_window_command,
    list_panes as list_panes_command, list_panes_all as list_panes_all_command,
    list_windows as list_windows_command, list_windows_all as list_windows_all_command,
    new_window_in_cwd as new_window_in_cwd_command, rename_window as rename_window_command,
    resize_window_height as resize_window_height_command, respawn_pane as respawn_pane_command,
    send_key as send_key_command, swap_window as swap_window_command, text_only_command,
    text_submit_command, tmux_submit_profile_for_harness,
};

pub mod input_diag;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxIoConfig {
    pub binary: String,
}

impl Default for TmuxIoConfig {
    fn default() -> Self {
        Self {
            binary: "tmux".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxIoError {
    Spawn { binary: String, message: String },
    Failed { code: Option<i32>, stderr: String },
    Utf8 { message: String },
}

impl fmt::Display for TmuxIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { binary, message } => {
                write!(f, "failed to spawn {binary}: {message}")
            }
            Self::Failed { code, stderr } => {
                write!(f, "tmux command failed with status {code:?}: {stderr}")
            }
            Self::Utf8 { message } => write!(f, "tmux output was not utf-8: {message}"),
        }
    }
}

impl Error for TmuxIoError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxPaneSelectionError {
    Query(TmuxIoError),
    Position(agent_doc_tmux::PanePositionError),
}

impl fmt::Display for TmuxPaneSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(err) => write!(f, "failed to query tmux panes: {err}"),
            Self::Position(err) => write!(f, "{err}"),
        }
    }
}

impl Error for TmuxPaneSelectionError {}

/// Return whether the current process is running inside a tmux client.
pub fn in_tmux() -> bool {
    std::env::var_os("TMUX").is_some()
}

pub trait TmuxCommandRunner {
    fn run(&self, command: &TmuxCommand) -> Result<String, TmuxIoError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessTmuxRunner {
    pub config: TmuxIoConfig,
}

impl ProcessTmuxRunner {
    pub fn new(config: TmuxIoConfig) -> Self {
        Self { config }
    }

    pub fn default_binary() -> Self {
        Self::new(TmuxIoConfig::default())
    }
}

impl TmuxCommandRunner for ProcessTmuxRunner {
    fn run(&self, command: &TmuxCommand) -> Result<String, TmuxIoError> {
        observation_cache::run_with_observation_cache(command, || {
            let output = Command::new(&self.config.binary)
                .args(command.args())
                .output()
                .map_err(|err| TmuxIoError::Spawn {
                    binary: self.config.binary.clone(),
                    message: err.to_string(),
                })?;

            tmux_output_to_string(output)
        })
    }
}

impl TmuxCommandRunner for tmux_router::Tmux {
    fn run(&self, command: &TmuxCommand) -> Result<String, TmuxIoError> {
        observation_cache::run_with_observation_cache(command, || {
            let output =
                self.cmd()
                    .args(command.args())
                    .output()
                    .map_err(|err| TmuxIoError::Spawn {
                        binary: "tmux".to_string(),
                        message: err.to_string(),
                    })?;

            tmux_output_to_string(output)
        })
    }
}

impl TmuxCommandRunner for tmux_router::IsolatedTmux {
    fn run(&self, command: &TmuxCommand) -> Result<String, TmuxIoError> {
        TmuxCommandRunner::run(&**self, command)
    }
}

fn tmux_output_to_string(output: Output) -> Result<String, TmuxIoError> {
    if !output.status.success() {
        return Err(TmuxIoError::Failed {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    String::from_utf8(output.stdout).map_err(|err| TmuxIoError::Utf8 {
        message: err.to_string(),
    })
}

pub fn pane_project_root(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
) -> Option<PathBuf> {
    let output = display_message_value(runner, Some(pane_id), "#{pane_current_path}")?;
    project_root_for_pane_current_path(&output)
}

/// The pane's raw working directory (`#{pane_current_path}`), *without* the
/// nearest-`.agent-doc` collapse that [`pane_project_root`] applies. Callers
/// that need the real git-repository boundary of the pane (e.g. to tell a
/// nested submodule pane apart from a superproject document) must use this, not
/// `pane_project_root`, because a submodule pane with no local `.agent-doc/`
/// collapses up to the superproject root under `find_project_root`.
pub fn pane_current_path(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
) -> Option<PathBuf> {
    let output = display_message_value(runner, Some(pane_id), "#{pane_current_path}")?;
    let trimmed = output.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

pub fn pane_pid(runner: &(impl TmuxCommandRunner + ?Sized), pane_id: &str) -> Option<u32> {
    display_message_value(runner, Some(pane_id), "#{pane_pid}")?
        .parse::<u32>()
        .ok()
}

pub fn target_session_name(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Option<String> {
    display_message_value_nonempty(runner, Some(target), "#{session_name}")
}

pub fn target_window_name(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Option<String> {
    display_message_value_nonempty(runner, Some(target), "#{window_name}")
}

pub fn target_window_id(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Option<String> {
    display_message_value_nonempty(runner, Some(target), "#{window_id}")
}

pub fn target_current_command(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Option<String> {
    display_message_value_nonempty(runner, Some(target), "#{pane_current_command}")
}

pub fn target_pane_id(runner: &(impl TmuxCommandRunner + ?Sized), target: &str) -> Option<String> {
    display_message_value_nonempty(runner, Some(target), "#{pane_id}")
}

pub fn current_pane_id(runner: &(impl TmuxCommandRunner + ?Sized)) -> Option<String> {
    display_message_value_nonempty(runner, None, "#{pane_id}")
}

pub fn current_pane_id_from_env_or_tmux(
    runner: &(impl TmuxCommandRunner + ?Sized),
) -> Option<String> {
    std::env::var("TMUX_PANE")
        .ok()
        .filter(|pane| !pane.trim().is_empty())
        .or_else(|| current_pane_id(runner))
}

pub fn socket_path(runner: &(impl TmuxCommandRunner + ?Sized)) -> Option<String> {
    display_message_value_nonempty(runner, None, "#{socket_path}")
}

pub fn show_message(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
    delay_ms: &str,
    message: &str,
) -> Result<(), TmuxIoError> {
    runner
        .run(&display_notification(target, delay_ms, message))
        .map(|_| ())
}

pub fn list_panes(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: Option<&str>,
    format: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&list_panes_command(target, format))
}

pub fn list_panes_all(
    runner: &(impl TmuxCommandRunner + ?Sized),
    format: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&list_panes_all_command(format))
}

pub fn pane_by_position(
    runner: &(impl TmuxCommandRunner + ?Sized),
    position: &str,
) -> Result<String, TmuxPaneSelectionError> {
    pane_by_position_in_target(runner, position, None, "current tmux window".to_string())
}

pub fn pane_by_position_in_window(
    runner: &(impl TmuxCommandRunner + ?Sized),
    position: &str,
    window: &str,
) -> Result<String, TmuxPaneSelectionError> {
    pane_by_position_in_target(
        runner,
        position,
        Some(window),
        format!("tmux window {window}"),
    )
}

fn pane_by_position_in_target(
    runner: &(impl TmuxCommandRunner + ?Sized),
    position: &str,
    target: Option<&str>,
    scope: String,
) -> Result<String, TmuxPaneSelectionError> {
    let text = list_panes(runner, target, agent_doc_tmux::TMUX_PANE_GEOMETRY_FORMAT)
        .map_err(TmuxPaneSelectionError::Query)?;
    agent_doc_tmux::select_pane_by_position(&text, position, &scope)
        .map_err(TmuxPaneSelectionError::Position)
}

pub fn list_windows(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: Option<&str>,
    format: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&list_windows_command(target, format))
}

pub fn has_window_named(
    runner: &(impl TmuxCommandRunner + ?Sized),
    session_name: &str,
    window_name: &str,
) -> bool {
    let Ok(output) = list_windows(runner, Some(session_name), "#{window_name}") else {
        return false;
    };
    output.lines().any(|line| line.trim() == window_name)
}

pub fn list_windows_all(
    runner: &(impl TmuxCommandRunner + ?Sized),
    format: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&list_windows_all_command(format))
}

pub fn new_window_in_cwd(
    runner: &(impl TmuxCommandRunner + ?Sized),
    cwd: &str,
    name: &str,
    command: &str,
) -> Result<(), TmuxIoError> {
    runner
        .run(&new_window_in_cwd_command(cwd, name, command))
        .map(|_| ())
}

pub fn kill_pane(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Result<(), TmuxIoError> {
    runner.run(&kill_pane_command(target)).map(|_| ())
}

pub fn kill_window(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Result<(), TmuxIoError> {
    runner.run(&kill_window_command(target)).map(|_| ())
}

pub fn respawn_pane(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
    command: &str,
) -> Result<(), TmuxIoError> {
    runner
        .run(&respawn_pane_command(target, command))
        .map(|_| ())
}

pub fn rename_window(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
    name: &str,
) -> Result<(), TmuxIoError> {
    runner.run(&rename_window_command(target, name)).map(|_| ())
}

pub fn resize_window_height(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
    height: &str,
) -> Result<(), TmuxIoError> {
    runner
        .run(&resize_window_height_command(target, height))
        .map(|_| ())
}

/// `#stashresizerestore`: re-fit a window to its clients, clearing any manual
/// `resize-window` height left behind by the stash-consolidation join workaround.
pub fn resize_window_to_clients(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Result<(), TmuxIoError> {
    runner
        .run(&agent_doc_tmux_commands::resize_window_to_clients(target))
        .map(|_| ())
}

pub fn swap_window(
    runner: &(impl TmuxCommandRunner + ?Sized),
    source: &str,
    target: &str,
) -> Result<(), TmuxIoError> {
    runner.run(&swap_window_command(source, target)).map(|_| ())
}

pub fn join_pane_guarded(
    tmux: &tmux_router::Tmux,
    src: &str,
    dst: &str,
    expected_session: &str,
    join_flag: &str,
) -> anyhow::Result<()> {
    tmux.ensure_pane_in_session(src, expected_session)?;
    tmux.ensure_pane_in_session(dst, expected_session)?;
    tmux_router::PaneMoveOp::new(tmux, src, dst).join(join_flag)
}

pub fn capture_pane(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&capture_pane_command(target))
}

pub fn capture_pane_with_ansi(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: &str,
) -> Result<String, TmuxIoError> {
    runner.run(&capture_pane_with_ansi_command(target))
}

pub fn run_command(
    runner: &(impl TmuxCommandRunner + ?Sized),
    command: &TmuxCommand,
) -> Result<(), TmuxIoError> {
    runner.run(command).map(|_| ())
}

pub fn send_key(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
    key: &str,
) -> Result<(), TmuxIoError> {
    run_command(runner, &send_key_command(pane_id, key))
}

pub fn send_key_logged(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
    key: &str,
    sink: input_diag::InputDiagSink<'_>,
    source: &str,
) -> Result<(), TmuxIoError> {
    input_diag::log_key_event(
        sink,
        source,
        &format!("pane:{pane_id}"),
        "tmux_send_key",
        key,
        key.len(),
        agent_doc_tmux_commands::input_diag::KeyEventMeta::default(),
    );
    send_key(runner, pane_id, key)
}

pub fn send_submitted_text_with_profile(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
    text: &str,
    profile: TmuxSubmitProfile,
) -> Result<(), TmuxIoError> {
    let split_delay_ms = profile.split_text_and_submit_delay_ms();
    if split_delay_ms == 0 {
        return run_command(runner, &text_submit_command(pane_id, text, profile));
    }

    run_command(runner, &text_only_command(pane_id, text))?;
    std::thread::sleep(std::time::Duration::from_millis(split_delay_ms));
    send_key(runner, pane_id, profile.submit_key())
}

pub fn send_submitted_text_logged(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
    text: &str,
    sink: input_diag::InputDiagSink<'_>,
    source: &str,
) -> Result<(), TmuxIoError> {
    let profile = tmux_submit_profile_for_harness("");
    log_submitted_text_profile(sink, source, pane_id, text, None, profile);
    send_submitted_text_with_profile(runner, pane_id, text, profile)
}

pub fn send_submitted_text_for_harness_logged(
    runner: &(impl TmuxCommandRunner + ?Sized),
    pane_id: &str,
    text: &str,
    harness: &str,
    sink: input_diag::InputDiagSink<'_>,
    source: &str,
) -> Result<(), TmuxIoError> {
    let profile = tmux_submit_profile_for_harness(harness);
    log_submitted_text_profile(sink, source, pane_id, text, Some(harness), profile);
    send_submitted_text_with_profile(runner, pane_id, text, profile)
}

fn log_submitted_text_profile(
    sink: input_diag::InputDiagSink<'_>,
    source: &str,
    pane_id: &str,
    text: &str,
    harness: Option<&str>,
    profile: TmuxSubmitProfile,
) {
    input_diag::log_text_submit(
        sink,
        source,
        &format!("pane:{pane_id}"),
        text,
        harness,
        profile.transform(),
        profile.submit_key(),
    );
}

pub fn display_message_value(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: Option<&str>,
    format: &str,
) -> Option<String> {
    Some(
        runner
            .run(&display_message(target, format))
            .ok()?
            .trim()
            .to_string(),
    )
}

pub fn display_message_value_nonempty(
    runner: &(impl TmuxCommandRunner + ?Sized),
    target: Option<&str>,
    format: &str,
) -> Option<String> {
    display_message_value(runner, target, format).filter(|value| !value.is_empty())
}

pub fn project_root_for_pane_current_path(output: &str) -> Option<PathBuf> {
    let current_path = output.trim();
    if current_path.is_empty() {
        return None;
    }
    let path = PathBuf::from(current_path);
    agent_doc_fs::find_project_root(&path).or(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_tmux_commands::capture_pane as capture_pane_command;
    use std::cell::RefCell;
    use std::fs;

    struct FakeRunner {
        response: String,
    }

    impl TmuxCommandRunner for FakeRunner {
        fn run(&self, _command: &TmuxCommand) -> Result<String, TmuxIoError> {
            Ok(self.response.clone())
        }
    }

    struct RecordingRunner {
        commands: RefCell<Vec<Vec<String>>>,
        response: String,
    }

    impl RecordingRunner {
        fn new() -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                response: String::new(),
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
        }
    }

    impl TmuxCommandRunner for RecordingRunner {
        fn run(&self, command: &TmuxCommand) -> Result<String, TmuxIoError> {
            self.commands.borrow_mut().push(command.args().to_vec());
            Ok(self.response.clone())
        }
    }

    #[test]
    fn runner_trait_accepts_pure_command_builders() {
        let runner = FakeRunner {
            response: "pane output".to_string(),
        };

        assert_eq!(
            runner.run(&capture_pane_command("%1")).unwrap(),
            "pane output"
        );
    }

    #[test]
    fn project_root_for_pane_current_path_returns_none_for_empty_output() {
        assert_eq!(project_root_for_pane_current_path("\n  \t"), None);
    }

    #[test]
    fn project_root_for_pane_current_path_falls_back_to_current_path() {
        let path = PathBuf::from("agent-doc-tmux-io-no-root");

        assert_eq!(
            project_root_for_pane_current_path(&format!("{}\n", path.display())),
            Some(path)
        );
    }

    #[test]
    fn pane_project_root_maps_current_path_to_agent_doc_root() {
        let base = std::env::temp_dir().join(format!(
            "agent-doc-tmux-io-root-{}-{}",
            std::process::id(),
            "pane-project-root"
        ));
        let nested = base.join("nested").join("work");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".agent-doc")).unwrap();
        fs::create_dir_all(&nested).unwrap();

        let runner = FakeRunner {
            response: format!("{}\n", nested.display()),
        };

        assert_eq!(pane_project_root(&runner, "%1"), Some(base.clone()));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn pane_pid_parses_trimmed_display_message_value() {
        let runner = FakeRunner {
            response: "4242\n".to_string(),
        };

        assert_eq!(pane_pid(&runner, "%1"), Some(4242));
    }

    #[test]
    fn pane_pid_rejects_invalid_display_message_value() {
        let runner = FakeRunner {
            response: "not-a-pid\n".to_string(),
        };

        assert_eq!(pane_pid(&runner, "%1"), None);
    }

    #[test]
    fn display_message_value_nonempty_trims_and_filters_blank_values() {
        let runner = FakeRunner {
            response: " agent-doc \n".to_string(),
        };
        assert_eq!(
            display_message_value_nonempty(&runner, Some("%1"), "#{window_name}"),
            Some("agent-doc".to_string())
        );

        let blank = FakeRunner {
            response: " \n".to_string(),
        };
        assert_eq!(
            display_message_value_nonempty(&blank, Some("%1"), "#{window_name}"),
            None
        );
    }

    #[test]
    fn show_message_accepts_notification_command_runner() {
        let runner = FakeRunner {
            response: String::new(),
        };

        show_message(&runner, "%1", "3000", "claimed").unwrap();
    }

    #[test]
    fn pane_identity_helpers_filter_blank_display_values() {
        let runner = FakeRunner {
            response: "%42\n".to_string(),
        };
        assert_eq!(current_pane_id(&runner), Some("%42".to_string()));
        assert_eq!(target_pane_id(&runner, "test"), Some("%42".to_string()));
        assert_eq!(target_window_id(&runner, "test"), Some("%42".to_string()));

        let blank = FakeRunner {
            response: "\n".to_string(),
        };
        assert_eq!(current_pane_id(&blank), None);
        assert_eq!(target_window_id(&blank, "test"), None);
    }

    #[test]
    fn capture_pane_with_ansi_runs_capture_command() {
        let runner = RecordingRunner::new();

        capture_pane_with_ansi(&runner, "%1").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["capture-pane", "-t", "%1", "-p", "-e"]]
        );
    }

    #[test]
    fn capture_pane_runs_plain_capture_command() {
        let runner = RecordingRunner::new();

        capture_pane(&runner, "%1").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["capture-pane", "-p", "-t", "%1"]]
        );
    }

    #[test]
    fn list_windows_runs_targeted_window_listing_command() {
        let runner = RecordingRunner::new();

        list_windows(&runner, Some("dev:"), "#{window_id}").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["list-windows", "-t", "dev:", "-F", "#{window_id}"]]
        );
    }

    #[test]
    fn has_window_named_matches_trimmed_window_names() {
        let runner = FakeRunner {
            response: "shell\n agent-doc \nlogs\n".to_string(),
        };

        assert!(has_window_named(&runner, "dev:", "agent-doc"));
        assert!(!has_window_named(&runner, "dev:", "missing"));
    }

    #[test]
    fn list_panes_runs_targeted_pane_listing_command() {
        let runner = RecordingRunner::new();

        list_panes(&runner, Some("@1"), "#{pane_id}").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["list-panes", "-t", "@1", "-F", "#{pane_id}"]]
        );
    }

    #[test]
    fn pane_by_position_selects_from_current_window_geometry() {
        let runner = FakeRunner {
            response: "%left 0 0 80 24\n%right 120 0 80 24\n".to_string(),
        };

        assert_eq!(pane_by_position(&runner, "left").unwrap(), "%left");
        assert_eq!(pane_by_position(&runner, "right").unwrap(), "%right");
    }

    #[test]
    fn pane_by_position_in_window_queries_targeted_geometry() {
        let runner = RecordingRunner {
            commands: RefCell::new(Vec::new()),
            response: "%top 0 0 160 12\n%low 0 12 160 36\n".to_string(),
        };

        assert_eq!(
            pane_by_position_in_window(&runner, "bottom", "@1").unwrap(),
            "%low"
        );
        assert_eq!(
            runner.commands(),
            vec![vec![
                "list-panes",
                "-t",
                "@1",
                "-F",
                agent_doc_tmux::TMUX_PANE_GEOMETRY_FORMAT,
            ]]
        );
    }

    #[test]
    fn list_panes_all_runs_all_pane_listing_command() {
        let runner = RecordingRunner::new();

        list_panes_all(&runner, "#{pane_id}").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["list-panes", "-a", "-F", "#{pane_id}"]]
        );
    }

    #[test]
    fn list_windows_all_runs_all_window_listing_command() {
        let runner = RecordingRunner::new();

        list_windows_all(&runner, "#{window_id}").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["list-windows", "-a", "-F", "#{window_id}"]]
        );
    }

    #[test]
    fn new_window_in_cwd_runs_named_window_command() {
        let runner = RecordingRunner::new();

        new_window_in_cwd(&runner, "/repo", "agent-doc", "agent-doc start plan.md").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec![
                "new-window",
                "-c",
                "/repo",
                "-n",
                "agent-doc",
                "agent-doc start plan.md"
            ]]
        );
    }

    #[test]
    fn kill_pane_runs_targeted_pane_kill_command() {
        let runner = RecordingRunner::new();

        kill_pane(&runner, "%7").unwrap();

        assert_eq!(runner.commands(), vec![vec!["kill-pane", "-t", "%7"]]);
    }

    #[test]
    fn kill_window_runs_targeted_window_kill_command() {
        let runner = RecordingRunner::new();

        kill_window(&runner, "@9").unwrap();

        assert_eq!(runner.commands(), vec![vec!["kill-window", "-t", "@9"]]);
    }

    #[test]
    fn respawn_pane_runs_targeted_respawn_command() {
        let runner = RecordingRunner::new();

        respawn_pane(&runner, "%4", "exec agent-doc start file.md").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec![
                "respawn-pane",
                "-k",
                "-t",
                "%4",
                "exec agent-doc start file.md"
            ]]
        );
    }

    #[test]
    fn rename_window_runs_targeted_rename_command() {
        let runner = RecordingRunner::new();

        rename_window(&runner, "@2", "agent-doc").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["rename-window", "-t", "@2", "agent-doc"]]
        );
    }

    #[test]
    fn resize_window_height_runs_targeted_resize_command() {
        let runner = RecordingRunner::new();

        resize_window_height(&runner, "@2", "1000").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["resize-window", "-t", "@2", "-y", "1000"]]
        );
    }

    #[test]
    fn swap_window_runs_targeted_swap_command() {
        let runner = RecordingRunner::new();

        swap_window(&runner, "@2", "@9").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["swap-window", "-s", "@2", "-t", "@9"]]
        );
    }

    #[test]
    fn send_key_runs_single_named_key_command() {
        let runner = RecordingRunner::new();

        send_key(&runner, "%1", "Enter").unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["send-keys", "-t", "%1", "Enter"]]
        );
    }

    #[test]
    fn send_key_logged_runs_single_named_key_command() {
        let runner = RecordingRunner::new();

        send_key_logged(
            &runner,
            "%1",
            "Enter",
            input_diag::InputDiagSink::new(None, |_file, _message| {}),
            "test.send_key",
        )
        .unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["send-keys", "-t", "%1", "Enter"]]
        );
    }

    #[test]
    fn send_submitted_text_with_profile_uses_single_command_without_split_delay() {
        let runner = RecordingRunner::new();

        send_submitted_text_with_profile(
            &runner,
            "%1",
            "agent-doc plan.md\n",
            TmuxSubmitProfile::new(),
        )
        .unwrap();

        assert_eq!(
            runner.commands(),
            vec![vec!["send-keys", "-t", "%1", "agent-doc plan.md", "Enter"]]
        );
    }

    #[test]
    fn send_submitted_text_for_harness_logged_uses_submit_profile() {
        let runner = RecordingRunner::new();

        send_submitted_text_for_harness_logged(
            &runner,
            "%1",
            "agent-doc plan.md\n",
            "codex",
            input_diag::InputDiagSink::new(None, |_file, _message| {}),
            "test.submit",
        )
        .unwrap();

        assert_eq!(
            runner.commands(),
            vec![
                vec!["send-keys", "-t", "%1", "agent-doc plan.md"],
                vec!["send-keys", "-t", "%1", "Enter"],
            ]
        );
    }

    #[test]
    fn send_submitted_text_with_profile_splits_text_and_submit_key_when_requested() {
        let runner = RecordingRunner::new();

        send_submitted_text_with_profile(
            &runner,
            "%1",
            "/new\n",
            TmuxSubmitProfile::with_split_text_submit_delay(1),
        )
        .unwrap();

        assert_eq!(
            runner.commands(),
            vec![
                vec!["send-keys", "-t", "%1", "/new"],
                vec!["send-keys", "-t", "%1", "Enter"],
            ]
        );
    }
}
