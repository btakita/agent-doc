---
name: feedback_pending_add_bracket_tag_sets_id_collides
description: "agent-doc --pending-add uses an embedded [#tag] as the item id (not the id= prefix); a bracket matching an existing review/backlog id collides + churns"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 732cc6f5-05aa-4ca9-8350-e06baeb4e31c
---

In `agent-doc write --pending-add "<text>"`, the item id comes from an embedded `[#tag]` token in the text — NOT from an `id=text` prefix (that prefix form is for `--pending-edit`; for `--pending-add` the leading `xxx=` becomes literal body text and the id auto-generates, e.g. `#6qtj`).

**Why:** caused a multi-step recovery on lazily-rs.md 2026-06-25/26. `--pending-add 'lzcrdtplane2=[#lzcrdtplane] Phase 2...'` made the backlog id `lzcrdtplane` (from the `[#lzcrdtplane]` bracket), colliding with the parent `agent:review` item `#lzcrdtplane`. The collision blocked `--done` ("id not found: lzcrdtplane2"), then a `--done lzcrdtplane` left a completed `[x]` item the reaper refused (same id in review) → `session-check INTERRUPTED`, and preflight repair reaped BOTH the backlog item and the review parent as an "already-done mirror".

**How to apply:** to set a clean phase id, embed the bracket alone: `--pending-add '[#lzcrdtplane3] op log + delta exchange ...'`. Per `write --help`, the canonical form is `id=<custom> ` (literal `id=` prefix + the id + a **trailing space** then the text) and `[#custom] ` is the compatibility form — both set the id. A bare `name=text` (no `id=`, no bracket, e.g. `provauth2=...`) does NOT set the id: it auto-generates one (`#q9w3`) and keeps `provauth2=` as literal body text (hit on agent-doc-bugs2.md 2026-06-26, filed 4 items as #f693/#q9w3/#fpyh/#818s). Do NOT reuse a bracket id that already exists in `agent:review`/`agent:backlog`. There is NO backlog-remove flag (only `--pending-clear` = nuke-all, or `--done` = false-archive) and `--pending-edit 'oldid=[#newid] text'` does NOT re-key (it embeds `[#newid]` as text → ugly double-bracket `[#oldid] [#newid]`), so a wrong auto-id can't be cleanly renamed mid-session — get the id right on the first `--pending-add`. If you don't care about the exact id, omit the bracket and accept the auto id; reference the phase in prose. Prefer one backlog id per actionable phase over a re-gated parent (the skill's guidance) — but that means the parent review umbrella item is what collides, so drop it and track phases as concrete backlog ids. See [[feedback_queue_go_marker_overrides_stop_churn]].
