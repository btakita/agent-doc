# Plan: preserve Codex sandbox and access policy across `/clear`

## Problem

After running `/clear` in a live Codex session launched by `agent-doc`, the next turn can lose the effective capability envelope the pane originally had. The user report is specific: file permissions and SSH access appear to drop after `/clear`, and the latest repro narrows that further to blocked DNS resolution for outbound hosts like `github.com`, even when the document/session was started with explicit Codex launch args such as `codex_args: -s danger-full-access`.

Current evidence in-tree points at a product gap rather than a documented feature:

- `runbooks/harness-invocation.md` still says Codex built-ins such as `/clear` are manual-only.
- The Codex supervisor path in `harness.rs` / `start.rs` assumes restarts happen through `codex resume --last`, inheriting the original session's launch contract.
- `specs/09-git-integration.md` already documents an important asymmetry: fresh `codex exec` accepts `--add-dir`, while `codex exec resume` does not, because agent-doc currently relies on resume inheriting writable roots from the original session.

If `/clear` creates a new effective conversation state inside Codex that does **not** retain the prior approvals / writable-root / network posture the way agent-doc expects, then the current restart and recovery model is incomplete.

## Desired behavior

`agent-doc` should support a `/clear`-style reset without silently degrading access policy.

Acceptable outcomes:

1. Preferred: a cleared Codex pane keeps the same effective sandbox, writable roots, network policy, and permission envelope as the original `agent-doc start`.
2. Fallback: if Codex cannot preserve that state across `/clear`, agent-doc must detect the loss and perform a deterministic fresh rebind/restart that reapplies the full launch contract.
3. Fail-closed alternative: if neither is possible, agent-doc should surface an explicit recovery instruction instead of leaving the user in a half-cleared session with weaker capabilities.

## Investigation plan

- [ ] Reproduce the report in a controlled Codex session started via `agent-doc start`, using a document with explicit `codex_args` and any required network/write-root setup. Capture the before/after behavior for file writes, git operations, DNS resolution, and an SSH command.
- [ ] Determine what `/clear` changes at the Codex layer:
  - same `session_id` vs new one
  - same Stop/UserPromptSubmit hook lifecycle vs reset state
  - preserved vs dropped sandbox / network / approval state
  - preserved vs dropped DNS/network reachability to previously working hosts
  - preserved vs dropped inherited writable roots (especially parent repo / submodule gitdirs that agent-doc adds on fresh exec)
- [ ] Audit the agent-doc assumptions that currently depend on resume inheritance:
  - `start.rs` Codex supervisor restart path
  - route/reopen behavior into already-live Codex panes
  - hook state in `codex_hook.rs`
  - any code/spec text that assumes `/clear` is purely cosmetic
- [ ] Decide the support model explicitly:
  - detect a post-`/clear` degraded session and replace it with a fresh `codex exec` under the same pane/session wiring, or
  - add a first-class `agent-doc` command that performs a safe "clear and relaunch with identical launch policy" flow instead of relying on raw Codex `/clear`
- [ ] Update the installed Codex instructions/runbooks so `/clear` is no longer treated as an unsupported manual corner if the binary now owns a safe recovery path.
- [ ] Add regression coverage around the chosen contract, including at minimum:
  - preserved Codex launch args after clear/rebind
  - preserved `CODEX_SANDBOX_NETWORK_DISABLED` policy handling
  - preserved hostname resolution / outbound reachability after clear/rebind when the original session had it
  - preserved multi-root write access assumptions for parent repos / submodules
  - fail-closed diagnostics when the original capability envelope cannot be proven
