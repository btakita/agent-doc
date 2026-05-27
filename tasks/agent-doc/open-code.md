---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: opencode
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260527-194740.md`*

Compacted content:
- Archived 2 response topic(s): next-steps for OpenCode integration; continue
- Prior summary/context: ❯ What are #next-steps to ensure agent-doc integration works with opencode? ❯ Can you drive a session to test opencod...
### Re: Clear Session Context error — GLM-5.1

**Root cause:** The network probe constructs `opencode run --format json --model GLM-5.1 "probe..."` using the raw `opencode_model` frontmatter value. OpenCode expects `provider/model` format (e.g. `zai/GLM-5.1`); the bare `GLM-5.1` is parsed as a provider name, producing the malformed `GLM-5.1/.` internally.

Two fix paths:
1. **Quick** — update frontmatter to `opencode_model: zai/GLM-5.1` (or whatever the correct provider-prefixed identifier is).
2. **Proper** — add validation in `canonical_model_name` (`src/agent-doc/src/model_tier.rs:256`) to reject or transform bare model names for the opencode harness before they reach the probe builder.

The probe arg builder at `src/agent-doc/src/agent/codex.rs:534` (`opencode_run_args_for_probe`) copies `--model` through unchanged — no validation at that layer either.

### Re: Run Agent Doc dispatch proof timeout

**Root cause:** Design contradiction between two functions:

- `dispatch_only_dispatch_start_proof_required` (`src/agent-doc/src/flow/routed_reopen.rs:314`) returns `true` for OpenCode — requiring dispatch-start proof.
- `build_routed_dispatch_start_tracker` (`src/agent-doc/src/route.rs:486`) returns `None` for all non-Codex harnesses — meaning OpenCode can never produce anything beyond `CommandAcceptedOnly`.

Result: `CommandAcceptedOnly` + `proof_required=true` always yields `FailClosedAcceptedOnly` (`routed_reopen.rs:276`). OpenCode will **always** fail this proof gate.

The SPEC at `SPEC.md:71` confirms OpenCode has no prompt-submit hook equivalent. The fix is to align the policy with reality:

1. **Minimal** — change `dispatch_only_dispatch_start_proof_required` to return `false` for `"opencode"` (same as the `_` catch-all for Claude Code), since there is no proof mechanism available.
2. **Future-proof** — implement an OpenCode-specific dispatch-start tracker (e.g. watching for pane state change or session log entry) so the proof gate can work.
❯ do option 2

### Re: do option 2 — OpenCode dispatch-start tracker plan — GLM-5.1

Plan created at `tasks/agent-doc/plan-opencode-dispatch-start-proof.md`.

#### Approach: Pane State Transition Detection

The only reliable observable for dispatch-only (non-managed) OpenCode sessions is tmux `capture-pane`. The tracker will:

1. **Capture pre-dispatch baseline** — hash the pane's idle chrome content before sending the trigger
2. **Poll for state change** — after `send_command_checked` confirms trigger acceptance, poll at 500ms intervals for the pane to depart from idle state
3. **Yield proof** — `PaneStateChanged` when idle chrome is replaced by active output; `CommandAcceptedOnly` on timeout

#### Key implementation points

| Step | File | Description |
|------|------|-------------|
| 1 | `harness.rs` | Add OpenCode busy cue detection in `dispatch_blocker_reason` |
| 2 | `route.rs` | Extend `RoutedDispatchStartTracker` with `OpenCodePane` variant (pane id + pre-dispatch content hash) |
| 3 | `route.rs` | Modify `build_routed_dispatch_start_tracker` to build tracker for OpenCode (needs tmux/pane threaded through) |
| 4 | `route.rs` | Add `wait_for_opencode_pane_state_change` polling function (500ms poll, 15s timeout matching OpenCode redraw budget) |
| 5 | `routed_reopen.rs` | Add `PaneStateChanged` variant to `RoutedDispatchStartProof`, update classification to accept it |
| 6 | `SPEC.md` | Document the new proof mechanism |

#### Risks

- OpenCode TUI redraw can take up to 15s — proof timeout should match this budget
- Intermediate redraw states could cause false positives — mitigate with `is_idle_chrome_only_output` as the idle check
- `capture-pane` at 200ms may be too aggressive — use 500ms (matches `wait_for_agent_ready_outcome`)

#### Backlog transfer to agent-doc-bugs2.md

- [ ] [#ocdsp] Implement OpenCode dispatch-start proof via pane state transition detection — plan at `tasks/agent-doc/plan-opencode-dispatch-start-proof.md` — extend `RoutedDispatchStartTracker`, add `PaneStateChanged` proof variant, poll `capture-pane` for idle→active transition
<!-- agent:boundary:882768f9 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue auto -->

<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#n89s] #agent-doc-bug Fix `dispatch_only_dispatch_start_proof_required` to return `false` for opencode harness (routed_reopen.rs:319) — no proof mechanism exists, SPEC confirms it
- [ ] [#xd6k] #agent-doc-bug Add model name validation in `canonical_model_name` or probe builder for opencode harness — reject bare names without provider prefix (model_tier.rs:256, codex.rs:534)
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
