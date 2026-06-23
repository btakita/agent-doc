> Extracted from SPEC.md — see [index](../SPEC.md)

# Commands

`specs/07-commands.md` remains the stable command-spec entrypoint, but the normative detail now lives in focused sibling specs instead of one 1000+ line file.

## Command spec map

| File | Scope |
|------|-------|
| [07-core-commands.md](07-core-commands.md) | Bootstrap, maintenance, inspection, transfer, backlog, and utility commands |
| [07-session-tmux-commands.md](07-session-tmux-commands.md) | `start` / `route` / `claim` / `sync` / `resync` / `session` and tmux-layout invariants |
| [07-closeout-commands.md](07-closeout-commands.md) | `commit` / `write` / `finalize` / `repair` / `preflight` / `session-check` closeout rules |
| [07-orchestration-commands.md](07-orchestration-commands.md) | `plan` / `orchestrate` / `agent:queue` lifecycle and queue consumption |

## Cross-file invariants

- Inside Codex, Claude Code, or OpenCode, bare `agent-doc <FILE>` remains the harness-native mode-aware entrypoint and must still durably capture the final response before write-back. In a normal shell, use an explicit subcommand such as `agent-doc run <FILE>`, `agent-doc route <FILE>`, or `agent-doc start <FILE>`.
- Response turns still close through the strict binary-owned path: write/merge -> snapshot/capture state -> commit -> `session-check`.
- `start`, `route`, `sync`, and `resync` share the same live-owner proof, startup-miss, stash, and session-resolution model. Do not fork those rules across separate specs.
- `plan`, `orchestrate`, sequential queue consumption, and normal response cycles all reuse the same `preflight` + `finalize` + `session-check` boundary.
- When command behavior changes, update the focused sibling spec instead of growing this index back into another monolith.
