---
name: feedback_complete_crashed_agent_doc_run_recovery_sequence
description: How to complete a crashed agent-doc run that left an in-progress submodule fix + corrupted session doc
metadata: 
  node_type: memory
  type: feedback
  originSessionId: cbc1cc61-e685-4fc3-852c-59e592f01b12
---

"Complete the last run on <doc>.md, the tmux session crashed" = finish + ship the in-progress submodule fix AND record the doc closeout. Sequence that worked (bugs2 #qmarkerauth+#pcwcrt → 0.34.61, 2026-06-26):

**Why:** a crashed run leaves the submodule with uncommitted fix code (verify `git -C src/agent-doc diff --stat`) and the session doc working-tree corrupted (the live repro of the very bug). The installed binary is still the OLD version, so the live route-owned supervisor has the bug until recycled.

**How to apply:**
1. **Verify the fix builds:** `make check` in the submodule. A code change that adds/removes `guard_`/`proof=`/`reason=`/`flow_reason=`/`accepted_only` tokens in a hot-path file (e.g. `git.rs`) trips `flowcore_hot_path_guard_and_proof_tokens_are_budgeted` (tests/test_cli.rs) — the gate is intentional; update the budget value in `flowcore_hot_path_token_budget` and re-audit its rationale comment. (Removing the post-commit revert tower dropped git.rs `reason=` 20→7.)
2. **Ship (`#deploy-just-do-it`):** bump all crate `Cargo.toml`+`pyproject.toml`, VERSIONS.md, `make install-full`, commit submodule, tag, push; `admin recycle <doc>` to move the live supervisor onto the new binary; bump the superproject `src/agent-doc` pointer in its OWN commit (stage only `src/agent-doc`), push superproject.
3. **Recover the corrupted doc:** `git checkout HEAD -- <doc>` (HEAD holds the correct committed exchange; the working-tree extra is pure corruption — verify the only diff is boundary-id + queue dup), then `agent-doc reset --from-current --preserve-session`.
4. **Record the closeout:** `write --commit` FAILS ("empty response — nothing to write") because the crashed cycle is abandoned (no captured response). Instead: Edit the `### Re:` closeout into the exchange before the boundary → `reset --from-current --preserve-session` (snapshot=working tree) → `agent-doc commit` (absorbs the `### Re:` insertion; guard `git show HEAD:<doc>|wc -c` doesn't regress).
5. **Strike + halt:** if you're the route-owned pane (`$TMUX_PANE` == supervisor's pane) and `queue: start`+`go`, ending the turn re-dispatches the unstruck head (churn). Strike the drained id (append to the `~~...~~` done line, remove from the `🚧` line), set `queue: stop` + remove `go` from the marker, `reset --from-current` + `commit`.
6. **Reap:** marking the backlog item `[x]` flips session-check to INTERRUPTED "completed tracked item after closeout: reap not persisted" — run `agent-doc preflight` (reaps to `.done.md`), then `reset --from-current --preserve-session` to clear the residual ~5-byte boundary snapshot/HEAD drift. session-check should end `ok` + `queue_continuation_required=false`.

CI flake note: a fast (~16s) CI `make check` exit-2 with `download of <crate> failed / curl failed / Connection reset by peer` is a transient crates.io registry flake, NOT a regression — `gh run rerun <id> --failed`. See [[feedback_operator_verify_do_agentdoable_release_substeps]].
