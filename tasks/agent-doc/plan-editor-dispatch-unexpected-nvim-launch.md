# Plan — editor dispatch unexpectedly launches `nvim` in the Codex pane

**Pending:** `#nvimrt` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft

## Problem

The simplified editor-triggered `Run Agent Doc` path is supposed to do one minimal thing:

- resolve the owning pane
- send the bare `agent-doc <FILE>` reopen
- press Enter

The new regression says `nvim` is sometimes opening in the tmux pane that is running Codex, and the user has to close `nvim` before the pane can accept routed commands again.

That is a contract violation for the editor path. A dispatch-only reopen must not inject extra control keys, replay stray shell/editor commands, or mutate the pane into another interactive program.

## Goal

Prove exactly how the pane reaches `nvim`, then harden the editor dispatch path so JetBrains and VS Code either:

- send only the bare `agent-doc <FILE>` command plus Enter, or
- fail closed before sending anything else

## Desired Behavior

- editor-triggered `Run Agent Doc` never sends `Ctrl+C`, `Ctrl+R`, `Escape`, `C-g`, or any other recovery/control sequence as part of `--dispatch-only`
- dispatch-only never reuses managed-route busy-pane recovery logic implicitly
- if the target pane is not dispatch-ready, the editor path surfaces a clear error instead of trying "helpful" key choreography
- the exact payload sent to tmux is auditable and stays the literal bare reopen command
- the same minimal contract holds in both JetBrains and VS Code

## Questions To Answer First

- Is `nvim` being launched by an explicit injected payload, or by a shell/history binding triggered by a control key?
- Is the editor path truly calling `route --dispatch-only`, or is some fallback still entering the managed recovery ladder?
- Can stale pane content, shell history, or a partial previous command cause the bare reopen to be transformed into `nvim`?
- Are any plugin-side helpers, tmux helpers, or supervisor helpers still sending extra keys around the reopen?

## Implementation Steps

- [ ] Reconstruct the exact same-document `agent-doc-bugs2.md` repro from local logs and scrollback, including the immediately preceding pane state before `nvim` appears
- [ ] Trace every send path used by JetBrains and VS Code `Run Agent Doc`:
  - plugin-side command assembly
  - `agent-doc route --dispatch-only`
  - tmux send/write helper
  - any retry or fallback path
- [ ] Define one strict dispatch-only invariant in code:
  - literal `agent-doc <FILE>`
  - one Enter
  - no control-sequence side effects
- [ ] Add an explicit guard so dispatch-only refuses to run if the only available recovery would require busy-pane control keys
- [ ] Audit managed-route helpers to ensure they cannot be reached from editor dispatch-only by accident
- [ ] Add logging/diagnostics that record the final outgoing payload and whether any control-key path was considered or skipped
- [ ] Add regression coverage for:
  - dispatch-only sends only the bare reopen command
  - dispatch-only does not call the busy-pane interrupt/recovery helpers
  - a non-ready pane fails closed instead of emitting extra keys
  - editor wrappers for JetBrains and VS Code both invoke the same minimal path

## Files Likely To Change

- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/harness.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TerminalUtil.kt`
- `src/agent-doc/editors/vscode/src/extension.ts`
- `src/agent-doc/specs/07-commands.md`
- `src/agent-doc/docs/guide/editor-integration.md`

## Exit Criteria

- JetBrains and VS Code `Run Agent Doc` only emit the bare reopen command plus Enter
- the `nvim`-launch repro is either eliminated or explained by one concrete guarded failure path
- dispatch-only no longer shares hidden control-key behavior with the heavier managed route path
