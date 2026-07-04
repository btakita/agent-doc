---
name: feedback_go_marker_churn_pause_resets_remove_go_set_stop
description: "go-mode no-op churn over struck heads — admin queue pause gets reset by the go marker; durable stop = remove `go` from marker + set queue:stop"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: b2dcaa07-a209-4abe-841f-861bfa76bb8c
---

When all real queue heads are struck/answered but `/agent-doc` keeps getting re-invoked with `queue_continuation_required=true` + `queue_drainable_head_count>0` whose `queue_prompts` are ALL `~~struck~~` (and `recent_noop_closeouts` climbing, e.g. 6→8→12), that is **go-mode no-op churn**, not real work — the `<!-- agent:queue preset="..." go -->` marker's `go` token force-resets `queue: start` every preflight (`#backlog-queue-attr-populates-in-go-mode`) and re-counts struck/backlog-mirror heads as drainable.

`agent-doc admin queue pause <FILE> --observed-generation <N>` only suppresses the **supervisor idle-watch** auto-injection; it does NOT stop the in-session loop, and the go-marker convergence **resets `queue_paused` back to null** on the next preflight (saw it flip `true`→`null` one cycle later). So pause does not hold.

**Durable stop (the only thing that holds):** edit the doc to (1) set frontmatter `queue: stop` AND (2) remove `go` from the `<!-- agent:queue preset="..." go -->` marker — BOTH, because `go` force-resets `stop`→`start`. There is no `agent-doc admin queue stop` (only pause/resume/drain) and no `write` flag for it, so it's a direct edit. Then `reset --from-current --preserve-session` + `agent-doc commit`. Verify: preflight shows `queue_active: null`, `queue_continuation_required: false`, `queue_drainable_head_count: 0`. Did this 2026-06-26 on equityfundingsource.md after 12 no-op closeouts. Editing managed queue state is normally discouraged while the supervisor is live, but here the supervisor idle-watch was already paused and the churn was unbounded. See [[feedback_queue_go_marker_overrides_stop_churn]] and [[feedback_halt_flooding_queue_admin_pause]].

**Whole-session caveat:** this happened atop a wedged JB **write** socket (`no_ack` every finalize → only `finalize --force-disk` / `agent-doc commit` land — see [[feedback_no_ack_write_socket_finalize_fails_writecommit_recovers]]). The real root fix is operator `restart-supervisor` / IDE restart; the queue:stop+remove-go just halts the churn meanwhile.
