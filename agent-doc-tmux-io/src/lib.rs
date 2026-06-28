//! Effectful tmux command execution adapter.
//!
//! This crate owns subprocess effects for tmux commands. It does not own
//! document authority, merge behavior, queue projection, or turn commits.

use std::error::Error;
use std::fmt;
use std::process::Command;

use agent_doc_tmux_commands::TmuxCommand;

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
        let output = Command::new(&self.config.binary)
            .args(command.args())
            .output()
            .map_err(|err| TmuxIoError::Spawn {
                binary: self.config.binary.clone(),
                message: err.to_string(),
            })?;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_tmux_commands::capture_pane;

    struct FakeRunner {
        response: String,
    }

    impl TmuxCommandRunner for FakeRunner {
        fn run(&self, _command: &TmuxCommand) -> Result<String, TmuxIoError> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn runner_trait_accepts_pure_command_builders() {
        let runner = FakeRunner {
            response: "pane output".to_string(),
        };

        assert_eq!(runner.run(&capture_pane("%1")).unwrap(), "pane output");
    }
}
