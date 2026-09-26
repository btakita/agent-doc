---
name: reference_pasted_report_prose_spawns_parallel_fanout
description: "A pasted status report mentioning 'a concurrent session' made agent-doc plan order a --mode parallel fan-out that created 3 git worktrees + 3 claude -p panes from quoted diff lines"
metadata:
  node_type: memory
  type: reference
---

Observed 2026-09-26 on `tasks/agent-doc/agent-doc-bugs.md`. Fixed in agent-doc
`0937d4bba`. The operator pasted another session's status report into the
exchange; executing the turn as SKILL step 0d requires spawned three real agents.

**The chain, each link measured:**

1. `agent_doc_diff::detect_orchestration_mode` joined **every** non-list line in
   the added block — *including prose below the list* — and substring-matched the
   bare adjective `"concurrent"`. The narrative said "points at a **concurrent**
   Codex session", so mode resolved `Parallel`.
2. `plan` emitted `required_commands: ["agent-doc orchestrate ... --mode parallel
   --from-exchange"]` **next to its own `parallelizable: false`** (`parallelizable
   = dispatch_candidate && repo_actions.len() > 1`; there was 1 repo action).
   SKILL step 0d makes `required_commands` mandatory.
3. `orchestrate --from-exchange` → `extract_tasks_from_text` takes the **last
   markdown list block** as the task list. The "tasks" were three *quoted diff
   lines*: `agent: codex -> agent: claude...`, `queue: go -> queue: stop`, `queue
   head review PR 567 again -> - (a preset ref)`.
4. `src/parallel.rs` created `.agent-doc/worktrees/<session>-{0,1,2}` + branches
   `deep/<session>/{0,1,2}` and three tmux panes running `claude -p`.

**How to recognise it:** `agent-doc orchestrate` appears to hang with no output —
it is `hrtimer_nanosleep` in the step-4 poll loop, single-threaded, ~0.3s CPU. If
you piped it through `tail`, you see nothing because tail buffers until EOF; pipe
to a file instead. Confirm with `git worktree list` and
`cat .agent-doc/worktrees/*/.agent-doc-prompt.txt` — the prompts are the giveaway.

**Kill order:** `kill -TERM` the orchestrate parent FIRST, then `tmux kill-pane`
each pane. Killing panes first lets orchestrate proceed to "Collecting results".
Then `git worktree remove --force` + `git branch -D` each — orchestrate never
cleans up (tracked as the orchworktreeleak backlog item).

**Why it matters beyond the wasted agents:** this is one instance of a class —
*code-owned guidance that orders an action the receiving session cannot or must
not take*. Two more found the same day: `session-check`'s
`realtime_steering_closeout_guidance` prescribed "run `agent-doc <FILE>` to
continue" while preflight's admission gate refused that exact trigger for a
three-way baseline/authority/disk split (see
[[reference_no_live_editor_reason_contradicts_live_editors_one]] for the sibling
family), and `stale_plugin`'s binary-stale branch ordered `make install` to every
session including those forbidden to install mid-session
([[feedback_no_mid_session_install_against_live_supervisor]]).

The fix pattern in all three: a shared predicate, not a second string. The split
classification plus its remedy now live in
`agent_doc_document::admission_divergence`, read by preflight *and* session-check,
so the two halves cannot drift apart again the way the preflight/session-check
settle-parity pair did.

Related: [[feedback_never_dispatch_concurrent_agents_into_one_repo]],
[[reference_agent_doc_recovery_index]].
