---
name: feedback_halt_flooding_queue_admin_pause
description: "Halt a runaway/flooding agent-doc queue via controller-mediated `admin queue pause`, not a doc Edit"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 2c3efeec-2218-4806-8f50-b9b0a6604404
---

When an agent-doc `go` queue floods the running harness session — repeatedly
submitting `agent-doc <FILE>` / `/agent-doc <FILE>` triggers into the live pane so
prompts stack in the harness input queue (operator must manually delete them) —
halt it with the **controller-mediated** command, NOT by editing the queue block:

```
agent-doc admin queue pause <FILE> --observed-generation <N> --reason "<why>"
```

It rejects without `--observed-generation`; the rejection prints
`current_generation=<N>` — re-run with that. Verify with
`agent-doc admin inspect <FILE>` → `queue_control=paused`. Resume with
`agent-doc admin queue resume <FILE>`.

**Why:** Editing the `agent:queue` block (or `queue_active` frontmatter) by hand
while the IPC supervisor is live races and corrupts state (see
[[feedback_operator_drive_items_on_go_queue_churn]]). `admin queue pause` goes
through the controller, so it is IPC-safe. The flooding itself is a real bug —
captured as `#qflood` with plan `tasks/agent-doc/plan-queue-dispatch-flood.md`
(continuation dispatcher likely doesn't gate on proven pane-idle / doesn't dedupe
an already-pending trigger; reproduced cross-harness on Claude Code + Codex).

**How to apply:** On a flooding/runaway queue, immediately `admin queue pause`,
then capture the bug to backlog and STOP — do not auto-loop (each loop adds more
flooding). Post-commit working-tree drift after a finalize in this state is the
`#postcommit-ipc-worktree-corruption` false-INTERRUPTED pattern — verify HEAD is
correct, then `git checkout HEAD -- FILE` (see
[[feedback_post_commit_worktree_drift_checkout_head_recovery]]).

**Scope of the pause (2026-06-12, agent-doc-bugs2.md):** `admin queue pause`
halts controller **dispatch** only — it does NOT flip the document's `queue:
start` / go-block, so `preflight` keeps reporting `queue_active:true
queue_trigger:auto` and `session-check` keeps printing
`queue_continuation_required=true` afterward. That is expected; the pause still
stops the re-invocation flood, and you (not the doc state) decide not to loop.
Don't chase `queue_active:false` — flipping the frontmatter by hand is the
race-corrupt path the pause exists to avoid.

**Self-churn on convergence mirror lines:** a `go`-mode queue can flood/churn on
its OWN preflight convergence output — each `preflight` re-appends backlog→queue
mirror lines (`- :round_pushpin: do [#fcc0]`, `- :round_pushpin: [#rtwbcast]`)
that `agent-doc commit` then refuses to fold into HEAD (`post-commit local
drift … leaving later local working-tree edits uncommitted`), so the next
preflight re-diffs them as `content_edit` user_intent changes. Net: no real user
prompt, but `no_changes:false` every cycle. When the active items are all
clean-session/operator-drive, this is the same flood — `admin queue pause` + STOP.

**Inspection-preflight hazard:** every bare `agent-doc preflight <FILE>` opens a
fresh `preflight_started` cycle; running several for inspection without a
following write leaves a dangling cycle and makes `session-check` report
INTERRUPTED. Close it with `agent-doc commit <FILE>` (→ `commit_already_current`).
Prefer `--json`-parsed single preflight reads over repeated inspection preflights.
