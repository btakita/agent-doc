//! Per-harness last-mile steering delivery (`#steeringharnessparity`).
//!
//! The steering derivation, the settle gate, and the delivery policy live in
//! the binary and are shared. Only the last mile differs per harness: how a
//! BUSY agent receives steering mid-turn, how an IDLE pane is woken, and which
//! non-interactive CLI (if any) can classify an inconclusive half-typed item.
//! This table is the single answer; installers, the supervisor, the instruction
//! surfaces, and the parity test all read it.

/// How a running (busy) agent receives steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusyDelivery {
    /// A harness `PostToolUse` hook (`agent-doc hook steering-post-tool-use`)
    /// injects settled steering after every tool call.
    PostToolUseHook,
    /// No mid-turn hook: the agent polls `agent-doc steering <FILE>` between
    /// long steps, and every agent-doc command it runs (preflight, finalize,
    /// write --commit, session-check) carries unsurfaced steering in its
    /// terminal report.
    PollAndCommandBoundary,
}

/// How an idle owning pane is woken for unsurfaced steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleDelivery {
    /// The route-owned supervisor submits the harness trigger into the owning
    /// tmux pane through the idle-queue dispatch, gated on that harness's
    /// prompt-ready detection (fail closed when readiness is unproven).
    SupervisorTrigger,
    /// No supervised pane exists to type into (an IDE-embedded agent).
    /// Steering waits for the agent's next agent-doc command.
    Unavailable { gap: &'static str },
}

/// The non-interactive CLI an optional completion classifier may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifierCli {
    pub program: &'static str,
    /// Arguments before the prompt. The prompt is passed as the final
    /// argument (or on stdin when `prompt_on_stdin`).
    pub args: &'static [&'static str],
    /// Flag that selects a model, when the harness allows choosing one.
    pub model_flag: Option<&'static str>,
    /// The smallest/fastest model the harness documents, if verified.
    pub default_model: Option<&'static str>,
    pub prompt_on_stdin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteeringDeliveryAdapter {
    pub harness: &'static str,
    pub busy: BusyDelivery,
    pub idle: IdleDelivery,
    /// `None`: no verified non-interactive CLI; the deterministic gate only.
    pub classifier: Option<ClassifierCli>,
}

/// Every harness agent-doc supports, by canonical binary name.
pub const SUPPORTED_HARNESSES: [&str; 5] = ["claude", "codex", "opencode", "grok", "cursor"];

/// The delivery adapter for `harness` (any spelling `normalize_harness_name`
/// or `HarnessConfig::from_agent_name` accepts). Unknown names fall back to
/// the Claude Code adapter, matching `HarnessConfig::from_agent_name`.
pub fn steering_delivery_adapter(harness: &str) -> SteeringDeliveryAdapter {
    let name = harness.trim().to_ascii_lowercase();
    match name.as_str() {
        "codex" => SteeringDeliveryAdapter {
            harness: "codex",
            busy: BusyDelivery::PostToolUseHook,
            idle: IdleDelivery::SupervisorTrigger,
            // `codex exec` is Codex's non-interactive mode; `--ephemeral`
            // keeps the probe out of session history, `-s read-only` gives it
            // no write access. No small model name is verified for this
            // install, so the configured Codex default model is used unless
            // `.agent-doc/config.toml` names one.
            classifier: Some(ClassifierCli {
                program: "codex",
                args: &[
                    "exec",
                    "--ephemeral",
                    "--skip-git-repo-check",
                    "-s",
                    "read-only",
                ],
                model_flag: Some("-m"),
                default_model: None,
                prompt_on_stdin: false,
            }),
        },
        "opencode" | "open-code" | "open_code" => SteeringDeliveryAdapter {
            harness: "opencode",
            busy: BusyDelivery::PollAndCommandBoundary,
            idle: IdleDelivery::SupervisorTrigger,
            // `opencode run [message..]`; `--pure` skips external plugins.
            // Its `-m` takes `provider/model`; no default is assumed.
            classifier: Some(ClassifierCli {
                program: "opencode",
                args: &["run", "--pure"],
                model_flag: Some("-m"),
                default_model: None,
                prompt_on_stdin: false,
            }),
        },
        "grok" | "grok-build" => SteeringDeliveryAdapter {
            harness: "grok",
            // Grok Build lifecycle hooks are notifications: stdout cannot
            // inject context into the running turn.
            busy: BusyDelivery::PollAndCommandBoundary,
            idle: IdleDelivery::SupervisorTrigger,
            // `grok -p <PROMPT>` is its single-turn headless mode.
            classifier: Some(ClassifierCli {
                program: "grok",
                args: &["-p"],
                model_flag: Some("-m"),
                default_model: None,
                prompt_on_stdin: false,
            }),
        },
        "cursor" | "cursor-agent" => SteeringDeliveryAdapter {
            harness: "cursor",
            busy: BusyDelivery::PollAndCommandBoundary,
            idle: IdleDelivery::Unavailable {
                gap: "Cursor runs inside the IDE, not in a route-owned tmux pane, so the \
                      supervisor has no composer to submit a wake into",
            },
            // No Cursor CLI is part of agent-doc's harness integration.
            classifier: None,
        },
        _ => SteeringDeliveryAdapter {
            harness: "claude",
            busy: BusyDelivery::PostToolUseHook,
            idle: IdleDelivery::SupervisorTrigger,
            // `claude -p` prints and exits; `--tools ""` disables every tool,
            // `--no-session-persistence` keeps the probe out of `--continue`.
            classifier: Some(ClassifierCli {
                program: "claude",
                args: &["-p", "--tools", "", "--no-session-persistence"],
                model_flag: Some("--model"),
                default_model: Some("haiku"),
                prompt_on_stdin: false,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parity: every supported harness has a busy AND an idle adapter, and a
    /// missing idle capability is a named gap rather than a silent skip.
    #[test]
    fn every_supported_harness_has_a_steering_delivery_adapter() {
        for harness in SUPPORTED_HARNESSES {
            let adapter = steering_delivery_adapter(harness);
            assert_eq!(adapter.harness, harness, "{harness} resolves to itself");
            if let IdleDelivery::Unavailable { gap } = adapter.idle {
                assert!(!gap.trim().is_empty(), "{harness} names its idle gap");
            }
        }
    }

    #[test]
    fn hook_harnesses_match_the_installed_post_tool_use_hooks() {
        assert_eq!(
            steering_delivery_adapter("claude").busy,
            BusyDelivery::PostToolUseHook
        );
        assert_eq!(
            steering_delivery_adapter("codex").busy,
            BusyDelivery::PostToolUseHook
        );
        for harness in ["opencode", "grok", "cursor"] {
            assert_eq!(
                steering_delivery_adapter(harness).busy,
                BusyDelivery::PollAndCommandBoundary,
                "{harness}"
            );
        }
    }

    #[test]
    fn supervised_tui_harnesses_wake_through_the_supervisor() {
        for harness in ["claude", "codex", "opencode", "grok", "grok-build"] {
            assert_eq!(
                steering_delivery_adapter(harness).idle,
                IdleDelivery::SupervisorTrigger,
                "{harness}"
            );
            assert!(crate::HarnessConfig::from_agent_name(harness).is_tui_harness());
        }
    }
}
