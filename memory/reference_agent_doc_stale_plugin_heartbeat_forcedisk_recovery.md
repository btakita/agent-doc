---
name: reference_agent_doc_stale_plugin_heartbeat_forcedisk_recovery
description: "agent-doc write wedged by a LIVE IDE with a stale plugin heartbeat (behind, pre-compaction buffer) — restart the disposable controller, force-disk write, manual commit past HEAD drift, reset to reconcile snapshot"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 24be4a68-bbf9-4da0-9209-75fe1c5b07b1
---

Recovery for a session doc where `preflight`/`plan`/`write` fail with `mismatched component` / `closing marker <!-- /agent:icebox --> without matching open` while **disk parses clean** (verify by copying the file out of `.agent-doc` and running `preflight` on the copy — clean copy ⇒ corruption is in the live editor/CRDT, not disk).

Distinct from [[reference_agent_doc_recovery_index]]'s 0.34.79 "phantom-editor demote-to-disk": here the JetBrains **IDE process is ALIVE** (`ps -p <pid>` shows `idea`), but `.agent-doc/plugin-owner/<hash>.json` `heartbeat_secs` is **minutes stale** and the IDE buffer holds the **pre-compaction** version (longer than disk) — the editor is *behind*, not ahead, so the auto-heal (which checks pid liveness) never fires and the supervisor keeps treating the wedged plugin as authority.

Steps that worked (0.34.80):
1. `reset --from-current --preserve-session --force-disk` — rebuilds clean CRDT/snapshot sidecars from disk (plain reset is refused: "editor/current authority differs from disk … rerun with --force-disk if the on-disk document is the intended authority").
2. `write --commit --force-disk` **still refused** by the editor-authority gate ("disk … was not used as write authority"). **Restart the disposable controller** first: `admin inspect` → find the project controller pid with **ppid 1** (NOT the route-owned supervisor, which is your own ancestor when you're in its pane — `kill-supervisor` correctly refuses it); `kill <controller_pid>` (supervisor respawns a fresh one that re-evaluates the stale lease and drops the wedged plugin from fanout).
3. Re-run `write --commit --force-disk` — patches now **apply to disk** but the git commit is refused: "refusing to auto-adopt committed historical response patchback … HEAD contains an out-of-band typed_component_drift mutation with response marker `### Re: … — gpt-5`" (a prior codex/gpt-5 stop-hook committed a response out-of-band; compaction archived it). `repair --apply-recovery` reports "clean — no recovery needed" and does NOT commit.
4. Confirm the disk-vs-HEAD deletions are ONLY already-archived exchange responses (`git diff | grep '^-.*### Re:'` should match the compaction Session Summary's archived topics; zero backlog/review/notes/done deletions), then **`git commit` the session doc manually** — the body already crossed the `agent-doc write` boundary, so this is finishing the commit the drift-guard declined, not hand-patching.
5. `session-check` then flags a small snapshot≠HEAD byte drift ("response patchback visible but never committed") even though `git status` is clean — reconcile with another `reset --from-current --preserve-session --force-disk`; `session-check` then returns `ok … no_drainable_work`.

Root signals: `.agent-doc/plugin-owner/<hash>.json` (stale `heartbeat_secs`), `.agent-doc/crdt-replica-events/<hash>.json` (`reason:fanout` timestamp == the frozen heartbeat ⇒ plugin got the fanout and never acked). Related: [[feedback_turn_unfinalized_live_editor_authority_blocks_commit_forcedisk_gitcommit]], [[reference_agent_doc_recovery_index]]. `--force-disk` is legitimate here because the IDE buffer is provably stale/behind and disk holds every operator prompt — but only after confirming no live operator edits are being dropped.
