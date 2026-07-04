---
name: feedback_check_git_before_deferring_repinned_clean_session
description: "A repeatedly re-pinned [clean-session] queue head may already be landed by the supervisor — check git before deferring again"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 16184d42-e2e5-45d2-afe1-4eb67186773c
---

When the operator re-pins a `[clean-session]` backlog id repeatedly (e.g. `#jbrestale` pinned 4×) and the in-session loop keeps deferring it (`queue_drainable_head_count: 0`, `queue_continuation_required: false`), **check git before reciting the clean-session deferral one more time** — the supervisor's `/clear`+re-dispatch (`#cleandrainsup`) drains `[clean-session]` heads to a *fresh* agent, so the code may already be committed/pushed while the exchange narrative never caught up.

**Why:** On 2026-06-14, `#jbrestale` (JB Run Agent Doc auto-restarts a stale-supervisor churn-stop) had been fully wired + tested + pushed at agent-doc `c7e7bd8e` by a supervisor-drained fresh agent at 12:06, but three prior exchange turns kept telling the operator "it's clean-session, restart the supervisor." The re-pin loop WAS the bug — nobody ran `git log`.

**How to apply:** Before deferring a re-pinned id again, run `git -C src/agent-doc log --oneline --grep <id>` (and `git merge-base --is-ancestor <commit> origin/main`). If the work landed: state that, give the operator the exact live-proof test + ops.log proof line, mark it `--done` (code shipped — the operator's standing directive is "I test + file bugs, you check logs," not "hold a gate open"), and push any committed-but-unpushed sibling commits (git-only, safe; never `cargo install` mid-session — `#no-mid-session-install`). Preflight's `semantic_completion_match` warnings are a weaker signal of the same thing. Related: [[feedback_operator_verify_do_agentdoable_release_substeps]], [[feedback_no_mid_session_install_against_live_supervisor]].
