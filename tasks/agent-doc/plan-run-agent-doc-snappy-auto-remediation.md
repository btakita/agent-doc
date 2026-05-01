# Plan: `Run Agent Doc` snappy auto-remediation for remaining routed Codex failures

**Pending:** `#snrun` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

`Run Agent Doc` still has two user-visible routed failure modes after the earlier `#runsm` hardening:

1. route can wait on a live Codex pane that is not actually dispatch-ready because the pane is stuck in an interactive shell substate such as `reverse-i-search`, then spend another recovery cycle on `agent-doc fix` before failing closed
2. route can report that the bare reopen was accepted in tmux but never consumed by Codex, even after the pane looked ready and the command was sent successfully

That means the route path is more precise than before, but it is still not meeting the plugin contract that `Run Agent Doc` should be fast and self-remediating.

## Root Causes To Prove

### 1. Dispatch-readiness still treats some interactive shell states as opaque busy panes

The readiness probe in `src/agent-doc/src/route.rs` only recognizes an empty Codex composer, placeholder prompts, and a small busy-cue set from `src/agent-doc/src/harness.rs`. It does not explicitly classify interactive shell/transient terminal states such as reverse history search, so route can burn its full idle timeout before it discovers that the pane needed a fresh restart or an explicit cancel/reset step instead.

### 2. Harness-consumption proof still depends on a single Codex hook signal

The routed reopen path now waits for `UserPromptSubmit` hook state to prove that Codex consumed `agent-doc <FILE>`. The latest repro shows a remaining gap where tmux accepted the reopen, but route never observed that exact hook advancement. That can happen because the pane was not truly ready, because the hook signal raced or was dropped, or because the route proof path has no second source of truth once the tmux send succeeds.

## Goal

Make same-document Codex reroutes either recover automatically in one bounded attempt or fail once with a stable reason that tells the user which stage broke, without long idle waits or ambiguous "accepted but never consumed" stalls.

## Implementation Steps

- [ ] Extend Codex busy/readiness classification to recognize interactive shell substates that should never be treated as a candidate live owner for reroute.
- [ ] Decide the automatic remediation for those substates:
  - cancel/reset in place when the state is provably recoverable, or
  - skip directly to one bounded fresh restart when it is not.
- [ ] Add a second routed-consumption proof after tmux send succeeds so route is not relying only on one hook update to distinguish "drafted", "submitted", and "submitted but no cycle started".
- [ ] Tighten routed diagnostics so the failure clearly says whether the pane was:
  - blocked in an interactive terminal substate
  - sent the reopen but never showed a Codex submission proof
  - submitted the reopen but never opened a new document cycle
- [ ] Add regression coverage for both new shapes using isolated tmux/Codex harness fixtures.
- [ ] Re-run the JetBrains `Run Agent Doc` repro against `tasks/agent-doc/agent-doc-bugs2.md` and confirm the route now recovers or fails once with a deterministic message.

## Files Likely To Change

- `src/agent-doc/src/harness.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/codex_hook.rs`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/VERSIONS.md`

## Exit Criteria

- route no longer spends the full idle wait on a pane stuck in reverse search or a similar non-dispatch terminal state
- routed reopen proof does not collapse every post-send failure into the same "accepted but never consumed" bucket
- JetBrains `Run Agent Doc` either self-recovers in one bounded attempt or returns one copyable, stage-specific failure
