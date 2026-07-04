---
name: feedback_no_mid_session_install_against_live_supervisor
description: "Don't cargo install/lib-install mid agent-doc session against a live stale route-owned supervisor — it makes the finalize client newer than the supervisor and triggers IPC drift that corrupts finalize."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e7dfecc6-6c99-459a-b62e-6ac8839149b9
---

In the agent-loop dogfooding session, do **NOT** `cargo build --release && cargo install --path . && agent-doc lib-install` mid-session while a live route-owned supervisor is still serving the document. Installing a newer binary makes your own `finalize` client newer than the running supervisor (e.g. pid 1562136), which can trigger a handoff/recycle mid-write — the recurring `#ipcdrift` / `live_prompt_drift_after_preflight` (exit-75) drift class that corrupts finalize and drops user-authored queue edits (`#queue-user-edit-overwrite`).

**Observed 2026-06-14:** I shipped a clean append-stable-only install (stashed `#supselfheal` WIP out first), but the install itself still skewed client > supervisor and my next `finalize` hit `live_prompt_drift_after_preflight` twice; recovered via `agent-doc commit` (auto-reconcile to HEAD, `#pcwc`). The operator had explicitly queued this exact warning, and my own prior-cycle response had already flagged it — I installed anyway. Don't repeat it.

**Why:** the route-owned supervisor and the finalize client must stay version-aligned; a one-binary skew is enough to race finalize.

**How to apply:** Defer install to *after* the session, OR restart/recycle the supervisor immediately after install (`agent-doc session restart-supervisor <FILE> --force`), OR do the build/install in a subagent that never touches the live supervisor. The durable product fix is `#supselfheal` (supervisor auto-recycle: blue/green `execve` preserve-child hot-reload at an idle boundary), which keeps client+supervisor aligned automatically — see [[feedback_ipc_drift_wedged_preparing_controllers]] and [[feedback_agent_doc_supervisor_self_race_finalize_block]].
