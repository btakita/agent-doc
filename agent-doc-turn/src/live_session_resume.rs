//! Pure live-session resume guard for the headless `run` path
//! (`#runchildresumesowner`).
//!
//! `agent-doc <FILE>` (and `agent-doc run <FILE>`) launches a headless harness
//! child and, when the document records a `resume` id for that harness, resumes
//! that conversation. On 2026-10-20 a live Claude Code coordinator ran it from
//! its own Bash tool. The document's `resume.claude` was the coordinator's own
//! conversation, so the child resumed the session that was running it. The
//! child had no bypass permissions, re-drove the coordinator's in-process
//! background subagents (each refused Bash/Read and emitted an operator
//! request), committed a fabricated "session restart" exchange entry through its
//! own `agent-doc respond`, and then failed the run path's second write of the
//! same turn.
//!
//! A conversation that is live in another process must never be resumed by a
//! second process. The proof this module accepts is the one the caller cannot
//! be wrong about: the harness exports the id of the session the command runs
//! inside (a subagent inherits its coordinator's), and that id equals the id the
//! run would resume. The I/O shell reads the environment; this module owns the
//! decision and the operator-facing refusal.

/// The environment variables a harness exports with the id of the session the
/// current process runs inside.
///
/// `harness` is the key from `harness_key_for_agent_name`. It is mapped the way
/// frontmatter maps a harness to its `resume` slot: Codex, OpenCode, and Grok
/// have their own slots, and every other agent (Claude Code, and any custom
/// agent such as a configured `claude` wrapper) resumes the `claude` slot, so
/// its resume id is a Claude Code conversation id. A harness with no stable
/// session-id export (OpenCode) returns an empty slice, so the guard never
/// fires on guesswork.
pub fn live_session_env_keys(harness: &str) -> &'static [&'static str] {
    match harness.trim() {
        "codex" => &["CODEX_THREAD_ID"],
        "opencode" | "open-code" | "open_code" => &[],
        "grok" | "grok-build" => &["GROK_SESSION_ID"],
        _ => &["CLAUDE_CODE_SESSION_ID"],
    }
}

/// A run that would resume a session that is live in the calling process tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSessionResumeConflict {
    pub harness: String,
    pub session_id: String,
    pub env_key: &'static str,
}

/// Decide whether resuming `resume_id` under `harness` would resume the live
/// session the caller runs inside.
///
/// `env` reads one environment variable. Blank values never match, and a run
/// with no resume id (a fresh fork) never conflicts: it starts a new
/// conversation instead of attaching to the live one.
pub fn live_session_resume_conflict(
    harness: &str,
    resume_id: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Option<LiveSessionResumeConflict> {
    let resume_id = resume_id.map(str::trim).filter(|id| !id.is_empty())?;
    live_session_env_keys(harness).iter().find_map(|key| {
        let live = env(key)?;
        (live.trim() == resume_id).then(|| LiveSessionResumeConflict {
            harness: harness.to_string(),
            session_id: resume_id.to_string(),
            env_key: key,
        })
    })
}

/// The refusal the run path prints, before it computes a diff, pre-commits, or
/// starts a cycle.
pub fn live_session_resume_refusal(
    document: &str,
    agent_name: &str,
    conflict: &LiveSessionResumeConflict,
) -> String {
    format!(
        "refusing to launch a headless `{agent_name}` child for {document}: this run would resume \
         harness session {session} (the document's `resume` id for {harness}), and that session \
         is LIVE — it is the session this command is running inside ({env_key}). A second process \
         resuming it would run beside the live conversation without its permission mode, re-drive \
         its background subagents, and write this document concurrently with it \
         (#runchildresumesowner). No diff, pre-commit, snapshot, cycle, or queue state was \
         touched. Recovery: answer the document from the live session's OWN turn — respond to \
         any pending edit there and close out with `agent-doc respond {document}` (or \
         `agent-doc write --commit {document}`); when nothing is left for this turn to answer, \
         end the turn and the next `/agent-doc {document}` cycle reads what remains. A subagent \
         reports back to its coordinator instead of answering the document. Use `agent-doc run \
         {document}` only from a plain shell where that session is not live.",
        session = conflict.session_id,
        harness = conflict.harness,
        env_key = conflict.env_key,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn resuming_the_session_the_caller_runs_inside_conflicts() {
        let conflict = live_session_resume_conflict(
            "claude-code",
            Some("ba846c4c-2e01"),
            env(&[("CLAUDE_CODE_SESSION_ID", "ba846c4c-2e01")]),
        )
        .expect("the incident shape must be refused");
        assert_eq!(conflict.env_key, "CLAUDE_CODE_SESSION_ID");
        assert_eq!(conflict.session_id, "ba846c4c-2e01");

        let codex = live_session_resume_conflict(
            "codex",
            Some("thread-1"),
            env(&[("CODEX_THREAD_ID", "thread-1")]),
        );
        assert_eq!(codex.map(|c| c.env_key), Some("CODEX_THREAD_ID"));
    }

    #[test]
    fn only_the_live_callers_own_resume_slot_conflicts() {
        // A different conversation is not the live caller.
        assert_eq!(
            live_session_resume_conflict(
                "claude-code",
                Some("other"),
                env(&[("CLAUDE_CODE_SESSION_ID", "live")]),
            ),
            None
        );
        // A fresh fork attaches to nothing.
        assert_eq!(
            live_session_resume_conflict(
                "claude-code",
                None,
                env(&[("CLAUDE_CODE_SESSION_ID", "live")]),
            ),
            None
        );
        // Blank ids never match each other.
        assert_eq!(
            live_session_resume_conflict(
                "claude-code",
                Some("  "),
                env(&[("CLAUDE_CODE_SESSION_ID", "  ")]),
            ),
            None
        );
        // The live session belongs to another harness than the one resumed.
        assert_eq!(
            live_session_resume_conflict(
                "codex",
                Some("live"),
                env(&[("CLAUDE_CODE_SESSION_ID", "live")]),
            ),
            None
        );
        // A custom agent resumes the `claude` slot, so its resume id is a
        // Claude Code conversation id and is guarded the same way.
        assert_eq!(
            live_session_resume_conflict(
                "mock",
                Some("live"),
                env(&[("CLAUDE_CODE_SESSION_ID", "live")]),
            )
            .map(|c| c.env_key),
            Some("CLAUDE_CODE_SESSION_ID")
        );
        // A harness without a session-id export never guesses.
        assert_eq!(
            live_session_resume_conflict("opencode", Some("live"), env(&[("OPENCODE", "live")])),
            None
        );
    }

    #[test]
    fn refusal_names_the_in_turn_closeout_and_never_the_run_path_as_recovery() {
        let conflict = LiveSessionResumeConflict {
            harness: "claude-code".to_string(),
            session_id: "s-1".to_string(),
            env_key: "CLAUDE_CODE_SESSION_ID",
        };
        let message = live_session_resume_refusal("plan.md", "claude", &conflict);
        assert!(message.contains("session s-1"), "{message}");
        assert!(message.contains("CLAUDE_CODE_SESSION_ID"), "{message}");
        assert!(message.contains("No diff, pre-commit"), "{message}");
        assert!(
            message.contains("`agent-doc respond plan.md`"),
            "the recovery is the live turn's own closeout: {message}"
        );
        assert!(
            !message.contains("run `agent-doc plan.md`"),
            "the refusal must not name the command it refuses: {message}"
        );
    }
}
