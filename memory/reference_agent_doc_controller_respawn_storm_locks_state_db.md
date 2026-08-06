---
name: reference_agent_doc_controller_respawn_storm_locks_state_db
description: "After admin recycle, controllers for one project root can respawn ~1 per 15-30s without exiting, contending on state.db until session-check returns \"database is locked\" — and reap-stale-controllers declines to act because they are not in the stuck-handoff state."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 5093a843-114e-42ac-93fd-a1a737a5a099
  modified: 2026-08-02T07:10:10.311Z
---

Observed 2026-08-02 on `/home/brian/work/btakita/agent-loop` (agent-doc 0.35.109),
shortly after an operator `agent-doc admin recycle`.

**Symptoms, in the order they appear:**

1. `agent-doc respond` / `write --commit` defers with the retained editor-write
   error (see [[reference_agent_doc_retained_editor_write_never_converges_commit_also_defers]]).
2. `session-check` then reports `canonical editor authority and disk projection
   diverge ... session-check is observing a non-recoverable projection state.
   The controller owns the next closeout attempt` and tells you NOT to rerun it.
3. Minutes later `session-check` prints
   `failed to persist controller model pressure in state.db: database is locked:
   Error code 5` and returns **no verdict** after ~20s.
4. `agent-doc admin inspect <FILE>` fails with
   `cannot launch project controller: project root "" is empty or not a directory`.

**The actual cause is a process leak, not corruption.** Check first:

```sh
head -c 16 .agent-doc/state.db | xxd     # `SQLite format 3` => NOT clobbered
pgrep -af 'controller serve --project-root' \
  | sed 's/.*--project-root \([^ ]*\).*/\1/' | sort | uniq -c | sort -rn
```

A healthy root shows **2** controllers. The wedged root showed 6 → 10 over a few
minutes, one new process every 15-30s, **none exiting**, zero zombies. They
contend on `state.db`, which is the whole explanation for the lock errors.

**`admin reap-stale-controllers` does NOT fix it** — it returned
`0 terminated, 1 kept`, because it only targets controllers wedged in
`Preparing`/`Promoted` past the stuck-handoff threshold. A respawn storm of
freshly-started controllers is a different state and it declines.

**Do NOT pkill by project root.** `agent-doc admin list` showed **seven** live
actors sharing that one root across seven panes/sessions (codex and claude-code
mixed). Killing controllers for the root kills every other session's controller
too. That is the hazard in
[[feedback_concurrent_sessions_force_push_shared_superproject]] in a different
shape. Run `admin list` before considering any kill so you can see who else is on
the root.

**How to apply:** when session-check starts reporting `database is locked`,
immediately count controllers per root rather than treating it as a document
problem. Confirm the state.db header is intact so you do not mistake it for
[[reference_agent_doc_state_db_header_clobbered_by_tracing_on_controller_recycle]].
If the count is climbing and other sessions share the root, stop and hand it to
the operator — the fix is theirs to choose, because every forceful option is
cross-session. Work already committed to git is unaffected; only the session
document's terminal commit is blocked.

Suspected trigger: `admin recycle` on a root whose installed binary is stale
relative to committed source, so the recycle target never settles. Unconfirmed.
