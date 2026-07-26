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
//! # Fails open, always
//!
//! A hook that breaks a turn is worse than a hook that does nothing. Every
//! unrecognized shape, missing file, and internal error returns quietly and lets
//! the prompt through unchanged. The only thing this hook can do is *add*
//! context.
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

/// The document an `agent-doc` invocation prompt targets, if any.
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
pub fn invoked_document(prompt: &str) -> Option<String> {
    let first_line = prompt.trim().lines().next()?.trim();
    let mut words = first_line.split_whitespace();
    let command = words.next()?;
    let command = command.strip_prefix('/').unwrap_or(command);
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
    Some(target.to_string())
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

    #[test]
    fn a_prompt_that_merely_mentions_the_command_is_not_a_trigger() {
        // The damage here is opening a cycle the operator did not ask for, so
        // only the first line counts.
        assert_eq!(invoked_document("can you run agent-doc tasks/plan.md?"), None);
        assert_eq!(
            invoked_document("Here is what I did:\nagent-doc tasks/plan.md"),
            None
        );
        assert_eq!(invoked_document("```\nagent-doc tasks/plan.md\n```"), None);
    }

    #[test]
    fn non_cycle_subcommands_own_their_own_flow() {
        for subcommand in ["claim", "compact", "reset", "repair", "session-check", "respond"] {
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
