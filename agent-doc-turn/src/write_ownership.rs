//! One ownership predicate shared by every "your write is retained" refusal.
//!
//! Three places tell an agent that a write is retained and forbid recovery, and
//! they live in three crates:
//!
//! | Site | Crate |
//! |---|---|
//! | `crdt_relay_pending_refusal` | `agent-doc-git-io` |
//! | authority/disk divergence INTERRUPT | `agent-doc-session-check-io` |
//! | `await_editor_replica_no_disk_write` | `agent-doc-document-realtime-io` |
//!
//! Until 0.35.123 none of them asked whether anything actually owned the write.
//! All three fired on `tasks/agent-doc/agent-doc-bugs2.md` on 2026-08-03 with the
//! newest cycle `committed` hours earlier and no capture retained, and each told
//! the session to stand down; every one was recovered by hand with
//! `agent-doc commit`. 0.35.123 fixed the `session-check` site only, so an agent
//! that reaches either of the other two first still obeys the wrong instruction.
//!
//! `crdt_relay_pending_refusal`'s own doc comment records the trap: it was made
//! one function "so all three call sites give the same instruction rather than
//! drifting into three dialects." Consolidating the *wording* did not stop the
//! *predicate* from diverging. So the predicate is the shared thing here, and the
//! wording is derived from it.
//!
//! The "do NOT re-send / force disk / `admin recycle` / `admin reload-lib`"
//! guidance stays intact wherever a real owner is found — it was written for a
//! real 2026-07-26 incident in which two sessions invented recoveries that each
//! perturbed the capture being awaited. Only the *unowned* case gains a remedy.
//!
//! This is the interim predicate for `#percellconverge`. Per-component ownership
//! replaces `cycle_open`/`retained_capture` with "does anything own **the
//! components this turn wrote**"; the call sites do not change when it does.

/// Whether anything durable owns a retained write.
///
/// Deliberately **not** keyed on editor attachment: `TransportProjection`'s
/// `editor_generation` is a one-way latch (`#editormodelmissing`) that reports an
/// editor as attached forever once one has ever attached, so a predicate that
/// trusted it would inherit the latch and answer "owned" for every document that
/// has ever been opened in an editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedWriteOwnership {
    /// A cycle is open for this document (`preflight_started`,
    /// `response_captured`, or `write_applied`).
    pub cycle_open: bool,
    /// A response capture is retained for this document.
    pub retained_capture: bool,
    /// The open cycle is specifically at `write_applied`
    /// (`#ownershipverdictdiverges`).
    ///
    /// Not every open phase is self-committing, and treating them alike is what
    /// made this predicate contradict `session-check`. At `preflight_started` and
    /// `response_captured` a state edge really does still fire. At
    /// `write_applied` the response write has ALREADY landed and only the
    /// terminal commit is outstanding — `session-check` calls that cycle
    /// INTERRUPTED, and only `agent-doc commit` advances it.
    pub write_applied: bool,
    /// The divergence is an unanswered edit to typed components this turn's
    /// write never produced — a queue strike, a backlog add — that the disk
    /// projection does not have yet (`#strandedremedydeadlock`).
    ///
    /// This is what distinguishes "the agent's write is stranded" from "an
    /// edit is sitting in the live buffer waiting for the next cycle". Both
    /// look identical to an ownership check — no cycle, no capture — but only
    /// the first is recoverable by committing.
    ///
    /// Deliberately **not** called an *operator* edit. The sites that set this
    /// compare components; none of them can prove who authored the difference,
    /// and the common non-operator case is an earlier `agent-doc write` whose
    /// own commit refused. Claiming authorship the predicate cannot establish
    /// is how a diagnostic starts lying — observed the same day this flag
    /// shipped, when a `--backlog-add-after` left exactly this drift and the
    /// message called it an operator edit.
    pub unanswered_edit: bool,
}

impl RetainedWriteOwnership {
    /// The shape a site reports when it has not looked. Kept explicit so a site
    /// cannot silently claim ownership it never proved: unproven reads as
    /// stranded, and the remedy names a recovery instead of forbidding one.
    pub const UNOWNED: Self = Self {
        cycle_open: false,
        retained_capture: false,
        write_applied: false,
        unanswered_edit: false,
    };

    pub const fn new(cycle_open: bool, retained_capture: bool) -> Self {
        Self {
            cycle_open,
            retained_capture,
            write_applied: false,
            unanswered_edit: false,
        }
    }

    /// [`Self::new`] for a caller that knows the open cycle's phase.
    pub const fn new_with_phase(
        cycle_open: bool,
        retained_capture: bool,
        write_applied: bool,
    ) -> Self {
        Self {
            cycle_open,
            retained_capture,
            write_applied,
            unanswered_edit: false,
        }
    }

    /// Record that the diverging components are an edit this turn's write did
    /// not produce. Only a site that has actually compared the components may
    /// set it; the default is `false`, so a site that has not looked keeps the
    /// conservative retained-write reading.
    pub const fn with_unanswered_edit(mut self, unanswered_edit: bool) -> Self {
        self.unanswered_edit = unanswered_edit;
        self
    }

    pub const fn verdict(self) -> RetainedWriteVerdict {
        // Checked FIRST: `write_applied` implies `cycle_open`, and the broader
        // arm would otherwise swallow it and promise a self-commit that never
        // comes.
        if self.write_applied {
            RetainedWriteVerdict::AwaitingTerminalCommit
        } else if self.cycle_open || self.retained_capture {
            RetainedWriteVerdict::Deferred
        } else if self.unanswered_edit {
            // Refines the unowned case only. With a durable holder the write
            // is genuinely deferred and the pending edit rides along with it;
            // it is the *unowned* shape the two readings collide on.
            RetainedWriteVerdict::UnansweredEditPending
        } else {
            RetainedWriteVerdict::Stranded
        }
    }

    pub const fn is_stranded(self) -> bool {
        matches!(self.verdict(), RetainedWriteVerdict::Stranded)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedWriteVerdict {
    /// Something durable holds the write, so a state edge will fire and the
    /// intent commits itself. Waiting is correct and every invented recovery
    /// perturbs the capture being awaited.
    Deferred,
    /// Nothing holds the write. No state edge can fire, so waiting never
    /// commits it — the visible edits are stranded, not deferred.
    Stranded,
    /// The response write already landed; only the terminal commit is
    /// outstanding (`#ownershipverdictdiverges`).
    ///
    /// Between the other two: the response is NOT lost, so re-sending it would
    /// duplicate work — but nothing is going to finish it either, so waiting is
    /// equally wrong. `agent-doc commit` is the one command that advances it.
    AwaitingTerminalCommit,
    /// Nothing owns a write because there is no write — the divergence is an
    /// unanswered edit to typed components this turn's write never produced
    /// (`#strandedremedydeadlock`).
    ///
    /// Indistinguishable from [`Self::Stranded`] by ownership alone, and the
    /// opposite instruction: committing a fresh unanswered edit would swallow
    /// the next turn's prompt, so every commit path refuses it on purpose.
    /// Answering it — running the document again — is what resolves it.
    UnansweredEditPending,
}

impl RetainedWriteVerdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deferred => "deferred",
            Self::Stranded => "stranded",
            Self::AwaitingTerminalCommit => "awaiting_terminal_commit",
            Self::UnansweredEditPending => "unanswered_edit_pending",
        }
    }

    /// Whether `agent-doc commit <FILE>` is the recovery this verdict names.
    ///
    /// `#strandedremedydeadlock`: naming a command is a promise that the command
    /// runs. `commit`'s already-current path used to hold an *independent*
    /// predicate — any non-exchange typed-component drift without a
    /// binary-owned retained-target proof was a terminal refusal — so the two
    /// commands could and did contradict each other. Observed 2026-08-09 on
    /// `tasks/agent-doc/agent-doc-bugs2.md`: `session-check` returned the
    /// `Stranded` verdict whose remedy is "run `agent-doc commit <FILE>`", and
    /// that exact command answered "refusing to close as already committed …
    /// typed-component drift without an exact binary-owned retained-target
    /// proof". An agent obeying the instruction faithfully had no next move,
    /// and the live queue drift it described could never be committed by
    /// anything.
    ///
    /// So the refusal is derived from the verdict rather than re-decided:
    /// `commit` reconciles exactly when the remedy sends the agent to it.
    /// `Deferred` is the one verdict that does not — something durable holds
    /// the write and commits it itself, and the 2026-07-26 "do NOT invent a
    /// recovery" guidance still governs there.
    pub const fn commit_is_the_named_recovery(self) -> bool {
        match self {
            Self::Deferred | Self::UnansweredEditPending => false,
            Self::Stranded | Self::AwaitingTerminalCommit => true,
        }
    }
}

/// The remedy every retained-write refusal appends, derived from one predicate.
///
/// `file` is the document path as the caller displays it; it is interpolated
/// into the commands so an agent can copy them verbatim.
pub fn retained_write_remedy(ownership: RetainedWriteOwnership, file: &str) -> String {
    match ownership.verdict() {
        RetainedWriteVerdict::Deferred => format!(
            "The capture is already durable and the same intent commits itself once delivery \
             converges — this is a deferral, not a lost response. Run \
             `agent-doc session-check {file}` once to observe the terminal state; do NOT \
             re-send the response, force disk, `admin recycle`, or `admin reload-lib`, all of \
             which disturb the capture being awaited"
        ),
        RetainedWriteVerdict::Stranded => format!(
            "NO cycle is open and NO response capture is retained, so nothing owns this write \
             and no state edge will fire — the visible edits are STRANDED, not deferred, and \
             waiting will not commit them. Recover from the pane that OWNS this session: \
             `agent-doc commit {file}`, or `agent-doc write --commit {file}` if an unwritten \
             response body remains. From any other pane both abort with `pane ownership \
             mismatch`"
        ),
        RetainedWriteVerdict::AwaitingTerminalCommit => format!(
            "The response write ALREADY LANDED and only the terminal commit is outstanding — \
             this is neither a lost response nor a self-completing deferral, and \
             `agent-doc session-check {file}` will report the cycle INTERRUPTED at \
             `write_applied`. Finish it from the pane that OWNS this session: \
             `agent-doc commit {file}`. Do NOT re-send the response (the body is already \
             durable and would duplicate), force disk, `admin recycle`, or `admin reload-lib`"
        ),
        RetainedWriteVerdict::UnansweredEditPending => format!(
            "NO write is retained — the response is already committed and the divergence is an \
             UNANSWERED DOCUMENT EDIT to typed components this turn's write never produced (a \
             queue or backlog line the live buffer has and the disk projection does not). It is \
             either operator steering or an earlier `agent-doc write` whose own commit refused; \
             this check compares components and cannot tell which, so it does not guess. Either \
             way it is not a failed closeout. Answer it: run `agent-doc {file}` to open the next \
             cycle, which reads that edit as its prompt and commits it. Do NOT run `agent-doc \
             commit {file}` — every commit path refuses a fresh unanswered edit on purpose, \
             because committing it would swallow the next turn's prompt — and do NOT force disk, \
             which clobbers the live edits"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_durable_holder_earns_the_deferral_claim() {
        assert_eq!(
            RetainedWriteOwnership::new(true, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(false, true).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(true, true).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new(false, false).verdict(),
            RetainedWriteVerdict::Stranded,
            "the 2026-08-03 shape: newest cycle committed hours earlier, zero retained captures"
        );
    }

    /// A site that has not looked must not inherit the deferral promise. This is
    /// the whole failure mode: the promise was unconditional, so every site made
    /// it for free.
    #[test]
    fn an_unproven_site_reads_as_stranded() {
        assert!(RetainedWriteOwnership::UNOWNED.is_stranded());
    }

    /// The owned case keeps the 2026-07-26 guidance verbatim: two sessions
    /// invented recoveries that each perturbed the capture being awaited.
    #[test]
    fn the_owned_remedy_still_forbids_every_invented_recovery() {
        let remedy = retained_write_remedy(RetainedWriteOwnership::new(true, false), "plan.md");

        assert!(remedy.contains("agent-doc session-check plan.md"));
        assert!(remedy.contains("deferral, not a lost response"));
        for invented in ["force disk", "admin recycle", "admin reload-lib", "re-send"] {
            assert!(
                remedy.contains(invented),
                "must explicitly rule out `{invented}`: {remedy}"
            );
        }
    }

    /// `#ownershipverdictdiverges`: `write_applied` is not self-committing, and
    /// lumping it in with the other open phases made this predicate contradict
    /// `session-check`. Observed three times on 2026-08-09 on
    /// `tasks/agent-doc/agent-doc-bugs2.md`: the refusal promised the intent
    /// "commits itself once delivery converges", `session-check` then reported
    /// INTERRUPTED at `write_applied`, and a single `agent-doc commit` finished
    /// it every time.
    #[test]
    fn write_applied_is_awaiting_a_terminal_commit_not_self_committing() {
        let applied = RetainedWriteOwnership::new_with_phase(true, true, true);
        assert_eq!(
            applied.verdict(),
            RetainedWriteVerdict::AwaitingTerminalCommit,
            "write_applied must not be swallowed by the broader cycle_open arm"
        );
        assert!(!applied.is_stranded(), "the response body IS durable");

        // The other open phases are unchanged: a state edge really does still fire.
        assert_eq!(
            RetainedWriteOwnership::new_with_phase(true, false, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
        assert_eq!(
            RetainedWriteOwnership::new_with_phase(false, true, false).verdict(),
            RetainedWriteVerdict::Deferred
        );
    }

    /// The remedy must name the command that actually recovers. Naming only
    /// `session-check` — an OBSERVATION — is what left the agent with an
    /// accurate diagnosis and no way to act on it.
    #[test]
    fn the_awaiting_commit_remedy_names_agent_doc_commit() {
        let remedy = retained_write_remedy(
            RetainedWriteOwnership::new_with_phase(true, true, true),
            "plan.md",
        );

        assert!(remedy.contains("agent-doc commit plan.md"));
        assert!(remedy.contains("ALREADY LANDED"));
        assert!(
            !remedy.contains("commits itself"),
            "promising a self-commit is the defect being fixed: {remedy}"
        );
        assert!(
            !remedy.contains("deferral, not a lost response"),
            "must not reuse the deferred verdict's phrase: {remedy}"
        );
        // Re-sending is still forbidden: the body is durable, so a re-send
        // duplicates rather than recovers.
        for invented in ["re-send", "force disk", "admin recycle", "admin reload-lib"] {
            assert!(remedy.contains(invented), "must rule out `{invented}`: {remedy}");
        }
    }

    /// `#strandedremedydeadlock`: the deadlock was two predicates disagreeing
    /// about one state — the remedy sent the agent to `agent-doc commit`, and
    /// `commit` refused. Naming the command and accepting it are now the SAME
    /// fact, so they cannot drift apart again: whatever `retained_write_remedy`
    /// prints, `commit_is_the_named_recovery` must agree with, for every
    /// verdict.
    #[test]
    fn commit_accepts_exactly_the_verdicts_whose_remedy_names_it() {
        for verdict in [
            RetainedWriteVerdict::Deferred,
            RetainedWriteVerdict::Stranded,
            RetainedWriteVerdict::AwaitingTerminalCommit,
            RetainedWriteVerdict::UnansweredEditPending,
        ] {
            let ownership = match verdict {
                RetainedWriteVerdict::Deferred => RetainedWriteOwnership::new(true, false),
                RetainedWriteVerdict::Stranded => RetainedWriteOwnership::UNOWNED,
                RetainedWriteVerdict::AwaitingTerminalCommit => {
                    RetainedWriteOwnership::new_with_phase(true, true, true)
                }
                RetainedWriteVerdict::UnansweredEditPending => {
                    RetainedWriteOwnership::UNOWNED.with_unanswered_edit(true)
                }
            };
            assert_eq!(ownership.verdict(), verdict, "fixture builds {verdict:?}");

            let remedy = retained_write_remedy(ownership, "plan.md");
            let names_commit = remedy.contains("`agent-doc commit plan.md`");
            let forbids_commit = remedy.contains("Do NOT run `agent-doc commit plan.md`");
            assert_eq!(
                names_commit && !forbids_commit,
                verdict.commit_is_the_named_recovery(),
                "{verdict:?}: the remedy and the commit-side predicate must not disagree: {remedy}"
            );
        }
    }

    /// The unowned case must name a recovery rather than forbid one. A session
    /// that obeys the owned wording here waits forever on an edge that cannot
    /// fire, which is exactly what happened three times on 2026-08-03.
    #[test]
    fn the_unowned_remedy_names_the_recovery_instead_of_forbidding_it() {
        let remedy = retained_write_remedy(RetainedWriteOwnership::UNOWNED, "plan.md");

        assert!(remedy.contains("STRANDED, not deferred"));
        assert!(remedy.contains("agent-doc commit plan.md"));
        assert!(remedy.contains("agent-doc write --commit plan.md"));
        assert!(
            !remedy.contains("do NOT"),
            "forbidding recovery is the defect being fixed: {remedy}"
        );
        assert!(
            !remedy.contains("deferral, not a lost response"),
            "the deferral promise must be earned, not asserted: {remedy}"
        );
    }
}
