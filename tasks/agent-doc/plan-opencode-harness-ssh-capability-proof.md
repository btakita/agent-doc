# Plan — add SSH capability proof for OpenCode harness

**Pending:** `#opencodesshproof`
**Status:** Draft

## Problem

The monsterrodholders.md agent-doc session declares `required_ssh_targets: [monsterrodholders-server]` in its frontmatter. When the harness is **Codex**, agent-doc runs `codex_capability_proof` probes (DNS + SSH + writable-root) before dispatch and logs results like:

```
codex_capability_proof status=proven network=proven ssh_targets=1 writable_roots=4
```

When the harness switches to **OpenCode** (generation 201+), **no capability proof runs at all**. The session log shows `opencode_start` directly with zero proof entries. The OpenCode child then discovers mid-response that it cannot SSH, producing confused output instead of a fail-closed diagnostic.

Evidence from session `a9421282-fd96-4943-9af5-3561ed5cb799`:
- Generations 194–197 (Codex): `codex_capability_proof status=proven ssh_targets=1`
- Generations 198–204 (OpenCode): no capability proof entries; multiple rapid restarts with no dispatch
- Host SSH works (`ssh monsterrodholders-server true` succeeds from outside agent-doc)
- Child agent sees `socket: Operation not permitted` — a kernel seccomp restriction inside the managed pane

The route layer already has `route_authoritative_actor_missing_codex_capability_proof` → `route_codex_capability_restart_fresh` logic, but only for Codex. OpenCode has no equivalent guard.

Related backlog items:
- `#codexcapstale` (plan-codex-managed-session-capability-proof.md) — Codex-only capability proof
- `#codexnonet` (plan-codex-network-sandbox-denial.md) — Codex network sandbox denial
- `#sshcut` (plan-agent-doc-prevents-ssh-connections.md) — broader SSH access investigation

## Goal

1. Generalize capability proof from Codex-only to **harness-agnostic**: any harness (Codex, OpenCode, Claude Code) that manages a session with `required_ssh_targets` or `codex_network_access` must prove those capabilities before dispatch.
2. Fail closed with a scope-specific diagnostic: distinguish "SSH unavailable inside managed pane" from "SSH unavailable on host".
3. Preserve the existing Codex proof path without regression.

## Implementation Steps

- [ ] Extract the capability-proof trait/interface from the Codex-specific implementation so the route/dispatch layer calls a harness-agnostic `prove_capabilities` method.
- [ ] Add an OpenCode capability proof implementation:
  - Run `ssh -o ConnectTimeout=5 -o BatchMode=yes <target> true` via the OpenCode harness's shell execution path.
  - Run a DNS/network probe if `codex_network_access: enabled` or `opencode_network_access: enabled` is declared.
  - Run writable-root probe for submodule paths.
  - Log results in the same format as `codex_capability_proof` (e.g., `opencode_capability_proof status=proven ssh_targets=1`).
- [ ] Gate dispatch on proof results for all harness types:
  - If proof passes, proceed to dispatch.
  - If proof fails, restart the managed pane fresh once, re-probe.
  - If fresh restart also fails, fail closed with diagnostic: `SSH unavailable inside managed {harness} pane — host SSH works but child cannot open socket. This is a sandbox restriction, not an SSH config issue.`
- [ ] Persist proof results in session/controller state so stale-proof detection works for OpenCode actors.
- [ ] Ensure the `route_authoritative_actor_missing_*_capability_proof` and `route_*_capability_restart_fresh` log events fire for OpenCode the same way they do for Codex.

## Regression Coverage

- [ ] OpenCode session with `required_ssh_targets` logs capability proof before first dispatch.
- [ ] OpenCode session where SSH probe fails logs the failure and retries fresh once.
- [ ] OpenCode session where fresh retry also fails produces a fail-closed diagnostic, not a confused agent response.
- [ ] Codex capability proof is not regressed (existing tests still pass).
- [ ] Session with no `required_ssh_targets` skips SSH proof for all harness types.

## Exit Criteria

- `agent-doc session status` reports capability proof results for OpenCode harnesses.
- The monsterrodholders.md workflow fails closed with a precise diagnostic when OpenCode cannot SSH, instead of dispatching the agent into a guaranteed failure.
- Codex sessions are unaffected.
