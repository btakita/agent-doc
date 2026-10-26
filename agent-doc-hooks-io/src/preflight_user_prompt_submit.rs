//! Run preflight in the binary when an `agent-doc <FILE>` prompt arrives
//! (`#preflightinbinary`).
//!
//! The cycle contract — diff, queue state, tier, model, claims — is computed
//! deterministically by the binary and owned by it end to end. Yet it reached
//! the agent only after a round trip the agent had to *remember* to make: the
//! trigger arrives as prompt text, the agent reads `SKILL.md`, and the agent
//! shells back to `agent-doc preflight`. An agent that forgets, or that answers
//! the prompt directly, silently skips repair, the prior cycle's commit, queue
//! convergence, and the baseline checkpoint.
//!
//! That is the same failure class as `#preflightsettleparity` and
//! `#closeoutwaitchurn`: a fact the system already has, gated behind a step
//! someone must remember. `UserPromptSubmit` fires at the moment the trigger
//! arrives, so the binary can run its own pipeline and hand the agent the
//! contract with the prompt — no round trip, and no way to skip it.
//!
//! # Best-effort hook, fail-closed workflow
//!
//! Every unrecognized non-agent-doc shape returns quietly so ordinary prompts
//! remain unaffected. The agent-doc workflow itself requires the emitted
//! contract and fails closed when it is absent; the model never recreates
//! admission by invoking or polling preflight.
//!
//! This module is the **decision** only. The effect — actually running preflight
//! — lives in the binary crate, because `agent-doc-commit-io` already depends on
//! this crate and preflight depends on commit, so calling preflight from here
//! would be a dependency cycle. Pure decision in the leaf, effect at the top.

use std::path::{Path, PathBuf};

/// Subcommands that are their own flow, not a session cycle.
///
/// `agent-doc compact <FILE>` and friends must not open a cycle, so a prompt
/// naming one is left alone.
const NON_CYCLE_SUBCOMMANDS: &[&str] = &[
    "claim",
    "compact",
    "reset",
    "repair",
    "recover",
    "session-check",
    "write",
    "respond",
    "finalize",
    "preflight",
    "plan",
    "status",
    "init",
    "skill",
    "install",
    "admin",
    "start",
    "route",
    "sync",
    "focus",
    "help",
];

/// A recognized session-cycle invocation.
///
/// Keeping the wrapper fact beside the target lets the hook adapter claim the
/// Claude loop lease before preflight reconciles the preceding continuation.
/// The parser remains effect-free; lease ownership stays at the binary edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDocInvocation {
    pub document: String,
    pub loop_wrapped: bool,
}

/// The `agent-doc` session-cycle invocation represented by a prompt, if any.
///
/// Recognizes the three harness spellings of the trigger — `/agent-doc <FILE>`
/// (Claude Code, OpenCode) and `agent-doc <FILE>` (Codex, direct) — and nothing
/// else. Deliberately strict:
///
/// - only the **first line** is considered, so a prompt that merely *mentions*
///   the command in prose or a code fence is not a trigger;
/// - the document must end in `.md`, because opening a cycle against the wrong
///   path is far worse than not opening one;
/// - a recognized non-cycle subcommand ([`NON_CYCLE_SUBCOMMANDS`]) returns
///   `None`, since those flows own their own boundaries;
/// - an unrecognized extra argument returns `None` rather than guessing.
///
/// `#loopcontractseal`: the queue auto-loop re-enters as `/loop agent-doc <FILE>`
/// (the `loop` skill re-arms with the original input prefixed by `/loop `, and
/// the agent-doc SKILL dispatches it as `Skill(loop, args: "agent-doc <FILE>")`).
/// That is the *same* trigger, so an optional leading `loop` wrapper — with the
/// optional interval token the skill accepts — is consumed before matching. Only
/// this one wrapper is recognized; every other leading word still means the
/// prompt merely mentions the command.
pub fn invoked_agent_doc(prompt: &str) -> Option<AgentDocInvocation> {
    let first_line = prompt.trim().lines().next()?.trim();
    let mut words = first_line.split_whitespace().peekable();
    let command = words.next()?;
    let command = command.strip_prefix('/').unwrap_or(command);
    let loop_wrapped = command == "loop";
    let command = if loop_wrapped {
        // An interval is optional in `/loop [interval] <prompt>`; a bare
        // `/loop 5m` with no prompt simply runs out of words below.
        if words.peek().is_some_and(|word| is_loop_interval(word)) {
            words.next();
        }
        let wrapped = words.next()?;
        wrapped.strip_prefix('/').unwrap_or(wrapped)
    } else {
        command
    };
    if command != "agent-doc" {
        return None;
    }
    let target = words.next()?;
    if NON_CYCLE_SUBCOMMANDS.contains(&target) {
        return None;
    }
    // Exactly one argument: the document. A trailing flag or second positional
    // means this is some other invocation, so leave it alone.
    if words.next().is_some() {
        return None;
    }
    if target.starts_with('-') || !target.ends_with(".md") {
        return None;
    }
    Some(AgentDocInvocation {
        document: target.to_string(),
        loop_wrapped,
    })
}

/// Compatibility projection for callers that only need the document.
pub fn invoked_document(prompt: &str) -> Option<String> {
    invoked_agent_doc(prompt).map(|invocation| invocation.document)
}

/// A no-argument session trigger: the first line is exactly
/// `/agent-doc` (Claude Code, Grok Build, OpenCode) or `agent-doc` (Codex).
///
/// The document is the one the current tmux pane claimed; resolving it is an
/// effect (registry read), so this leaf only recognizes the shape and
/// [`with_resolved_document`] rewrites it into the ordinary `<FILE>` trigger.
/// Every downstream parser, session binding, and same-turn repeat check then
/// sees one canonical prompt naming the resolved document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BareAgentDocTrigger {
    /// `true` for the slash spelling, so the rewrite keeps the harness form.
    pub slash: bool,
    /// Lines follow the trigger line (a trigger plus prompt body, or a paste).
    pub multi_line: bool,
}

/// Recognize the no-argument trigger. Deliberately as strict as
/// [`invoked_agent_doc`]: only the first line counts, and it must be the bare
/// command with nothing else on it. A `/loop` wrapper is not accepted; the
/// queue auto-loop re-enters with the resolved `<FILE>` the contract named.
pub fn bare_agent_doc_trigger(prompt: &str) -> Option<BareAgentDocTrigger> {
    let trimmed = prompt.trim();
    let mut lines = trimmed.lines();
    let first_line = lines.next()?.trim();
    let slash = match first_line {
        "/agent-doc" => true,
        "agent-doc" => false,
        _ => return None,
    };
    Some(BareAgentDocTrigger {
        slash,
        multi_line: lines.next().is_some(),
    })
}

/// Rewrite a bare trigger into the `<FILE>` trigger for `document`, keeping
/// the harness spelling and any following prompt body verbatim.
///
/// Returns `None` when the prompt is not a bare trigger, or when `document`
/// cannot round-trip through [`invoked_document`] (for example a path with
/// whitespace), so the caller refuses instead of admitting a different file.
pub fn with_resolved_document(prompt: &str, document: &str) -> Option<String> {
    let trigger = bare_agent_doc_trigger(prompt)?;
    let command = if trigger.slash {
        "/agent-doc"
    } else {
        "agent-doc"
    };
    let trimmed = prompt.trim_start();
    let rest = match trimmed.find('\n') {
        Some(index) => &trimmed[index..],
        None => "",
    };
    let rewritten = format!("{command} {document}{rest}");
    (invoked_document(&rewritten).as_deref() == Some(document)).then_some(rewritten)
}

/// A `/loop` interval token: `<digits><s|m|h|d>`, per the `loop` skill's own
/// leading-token rule.
fn is_loop_interval(word: &str) -> bool {
    let Some(digits) = word.strip_suffix(['s', 'm', 'h', 'd']) else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// Resolve the document against the harness-reported working directory.
///
/// Returns `None` when the path does not resolve to an existing file — a
/// mistyped path must not open a cycle somewhere unexpected.
pub fn resolve_document(cwd: &Path, target: &str) -> Option<PathBuf> {
    let candidate = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        cwd.join(target)
    };
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_harness_spelling_of_the_trigger() {
        assert_eq!(
            invoked_document("/agent-doc tasks/plan.md"),
            Some("tasks/plan.md".to_string()),
            "Claude Code / OpenCode slash form"
        );
        assert_eq!(
            invoked_document("agent-doc tasks/plan.md"),
            Some("tasks/plan.md".to_string()),
            "Codex / direct form"
        );
        assert_eq!(
            invoked_document("  agent-doc /abs/tasks/plan.md  \n"),
            Some("/abs/tasks/plan.md".to_string()),
            "surrounding whitespace and absolute paths"
        );
    }

    /// `#loopcontractseal`: the queue auto-loop's own re-entry shape. Without
    /// this the drain silently loses binary-owned admission — the hook emits no
    /// cycle contract, and the skill (correctly) fails closed instead of
    /// draining, so the queue stalls with work still on it.
    #[test]
    fn the_queue_auto_loop_reentry_is_the_same_trigger() {
        assert_eq!(
            invoked_agent_doc("/loop agent-doc tasks/plan.md"),
            Some(AgentDocInvocation {
                document: "tasks/plan.md".to_string(),
                loop_wrapped: true,
            }),
            "the effect adapter must be able to claim the loop lease before preflight"
        );
        assert_eq!(
            invoked_document("/loop agent-doc tasks/plan.md"),
            Some("tasks/plan.md".to_string()),
            "ScheduleWakeup re-arms with the original input prefixed by `/loop `"
        );
        assert_eq!(
            invoked_document("loop agent-doc tasks/plan.md"),
            Some("tasks/plan.md".to_string()),
            "bare `loop` wrapper"
        );
        assert_eq!(
            invoked_document("/loop agent-doc /abs/tasks/plan.md"),
            Some("/abs/tasks/plan.md".to_string()),
        );
        assert_eq!(
            invoked_document("/loop /agent-doc tasks/plan.md"),
            Some("tasks/plan.md".to_string()),
            "the wrapped trigger may keep its own slash"
        );
        for interval in ["5m", "30s", "2h", "1d"] {
            assert_eq!(
                invoked_document(&format!("/loop {interval} agent-doc tasks/plan.md")),
                Some("tasks/plan.md".to_string()),
                "fixed-interval loop form"
            );
        }
    }

    /// The wrapper does not loosen any other rule: subcommands, extra
    /// arguments, non-`.md` targets, and unrelated wrappers stay rejected.
    #[test]
    fn the_loop_wrapper_does_not_widen_admission() {
        assert_eq!(invoked_document("/loop"), None);
        assert_eq!(invoked_document("/loop 5m"), None);
        assert_eq!(invoked_document("/loop agent-doc"), None);
        assert_eq!(invoked_document("/loop tasks/plan.md"), None);
        assert_eq!(
            invoked_document("/loop agent-doc compact tasks/plan.md"),
            None,
            "a non-cycle subcommand is still not a cycle"
        );
        assert_eq!(
            invoked_document("/loop agent-doc tasks/plan.md --force"),
            None
        );
        assert_eq!(invoked_document("/loop agent-doc tasks/plan.txt"), None);
        assert_eq!(
            invoked_document("/loop /loop agent-doc tasks/plan.md"),
            None,
            "exactly one wrapper — nested loops are not a trigger shape"
        );
        assert_eq!(
            invoked_document("/schedule agent-doc tasks/plan.md"),
            None,
            "only the loop wrapper is recognized"
        );
        assert_eq!(
            invoked_document("loop over agent-doc tasks/plan.md"),
            None,
            "prose after `loop` is not the trigger"
        );
    }

    #[test]
    fn a_direct_trigger_is_not_mislabeled_as_loop_owned() {
        assert_eq!(
            invoked_agent_doc("agent-doc tasks/plan.md"),
            Some(AgentDocInvocation {
                document: "tasks/plan.md".to_string(),
                loop_wrapped: false,
            })
        );
    }

    #[test]
    fn a_prompt_that_merely_mentions_the_command_is_not_a_trigger() {
        // The damage here is opening a cycle the operator did not ask for, so
        // only the first line counts.
        assert_eq!(
            invoked_document("can you run agent-doc tasks/plan.md?"),
            None
        );
        assert_eq!(
            invoked_document("Here is what I did:\nagent-doc tasks/plan.md"),
            None
        );
        assert_eq!(invoked_document("```\nagent-doc tasks/plan.md\n```"), None);
    }

    #[test]
    fn non_cycle_subcommands_own_their_own_flow() {
        for subcommand in [
            "claim",
            "compact",
            "reset",
            "repair",
            "session-check",
            "respond",
        ] {
            assert_eq!(
                invoked_document(&format!("/agent-doc {subcommand} tasks/plan.md")),
                None,
                "{subcommand} must not open a cycle from the hook"
            );
        }
    }

    #[test]
    fn anything_unrecognized_is_left_alone() {
        assert_eq!(invoked_document(""), None);
        assert_eq!(invoked_document("agent-doc"), None);
        assert_eq!(invoked_document("/agent-doc"), None);
        assert_eq!(invoked_document("agent-doc --help"), None);
        assert_eq!(invoked_document("agent-doc tasks/plan.txt"), None);
        assert_eq!(invoked_document("agent-docx tasks/plan.md"), None);
        assert_eq!(
            invoked_document("agent-doc tasks/plan.md --force"),
            None,
            "an extra argument means some other invocation"
        );
        assert_eq!(invoked_document("other-tool tasks/plan.md"), None);
    }

    /// The bare entrypoint is recognized in both harness
    /// spellings and nothing looser.
    #[test]
    fn the_bare_entrypoint_is_recognized_in_both_spellings() {
        assert_eq!(
            bare_agent_doc_trigger("/agent-doc"),
            Some(BareAgentDocTrigger {
                slash: true,
                multi_line: false
            })
        );
        assert_eq!(
            bare_agent_doc_trigger("  agent-doc  \n"),
            Some(BareAgentDocTrigger {
                slash: false,
                multi_line: false
            })
        );
        assert_eq!(
            bare_agent_doc_trigger("agent-doc\ndo #fix now"),
            Some(BareAgentDocTrigger {
                slash: false,
                multi_line: true
            })
        );
        for not_bare in [
            "",
            "agent-doc tasks/plan.md",
            "/agent-doc claim tasks/plan.md",
            "/loop agent-doc",
            "agent-docx",
            "run agent-doc",
            "Here is the log:\nagent-doc",
        ] {
            assert_eq!(bare_agent_doc_trigger(not_bare), None, "{not_bare:?}");
        }
    }

    #[test]
    fn a_bare_trigger_rewrites_into_the_file_trigger_for_every_parser() {
        let slash = with_resolved_document("/agent-doc", "tasks/plan.md").unwrap();
        assert_eq!(slash, "/agent-doc tasks/plan.md");
        assert_eq!(invoked_document(&slash), Some("tasks/plan.md".to_string()));

        let codex = with_resolved_document("agent-doc\ndo #fix now", "tasks/plan.md").unwrap();
        assert_eq!(codex, "agent-doc tasks/plan.md\ndo #fix now");
        assert_eq!(invoked_document(&codex), Some("tasks/plan.md".to_string()));

        assert_eq!(
            with_resolved_document("agent-doc", "tasks/my plan.md"),
            None,
            "a path that cannot round-trip must refuse, not admit another file"
        );
        assert_eq!(
            with_resolved_document("agent-doc tasks/plan.md", "tasks/other.md"),
            None,
            "only a bare trigger is rewritten"
        );
    }

    #[test]
    fn a_path_that_does_not_exist_never_opens_a_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("plan.md");
        std::fs::write(&present, "# Session\n").unwrap();

        assert_eq!(resolve_document(dir.path(), "plan.md"), Some(present));
        assert_eq!(resolve_document(dir.path(), "missing.md"), None);
        assert_eq!(
            resolve_document(dir.path(), "."),
            None,
            "a directory is not a session document"
        );
    }
}
