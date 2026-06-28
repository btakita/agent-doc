# Plan: restart-supervisor re-associates the wrong document (#suprestassoc)

## Problem (operator repro)
"When I restarted the equityfundingsource.md supervisor, equityfundingsource.md was re-associated with the agent-doc-bugs2.md claude session."

I.e. `agent-doc session restart-supervisor tasks/professional/equityfundingsource.md` caused EFS's authoritative actor binding to point at the pane/session that owns `agent-doc-bugs2.md`, hijacking the wrong doc.

## Suspected cause
Multiple documents share ONE route-owned host supervisor / project controller (project-scoped). On restart, the actor re-register / supervisor-lease path likely keys the re-association on the supervisor/pane rather than on the (document_id, session_id) pair, so the restarted supervisor adopts the requesting pane's session for a *different* document. Candidate sites: `session_actor_cmd.rs` (restart-supervisor), `route::authoritative_actor` / `authoritative_actor_binding`, and the supervisor-lease register/replace path in `project_controller.rs` (a newer register/heartbeat replacing a `Closed` actor for the same pane — `#README:271` — may be matching too broadly across documents).

## Investigation steps
1. Live repro under ops-log capture: restart EFS supervisor, diff `admin list` / `authoritative_actor_binding` for both docs before/after; capture the `actor register/replace` + `supervisor_lease` log lines.
2. Confirm whether the binding key conflates pane/session across documents.

## Fix direction
Scope actor re-association strictly to the (document_id, generation) being restarted; never let a restart adopt another document's session/pane binding even when they share a project controller. Add a regression (SimWorld or unit) that restarts doc A's supervisor and asserts doc B's binding is untouched.

## Note
Needs a live editor/pane repro to confirm + verify (`[operator-verify]` for the final proof); the binding-scope fix + regression are agent-doable.
