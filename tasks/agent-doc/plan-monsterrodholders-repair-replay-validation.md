---
title: Validate monsterrodholders repair replay after backlog alias-tag normalization
created: 2026-05-01
status: open
---

# Goal

Confirm that the repaired pending/backlog normalization path works end-to-end in the live `src/boost-client/tasks/monsterrodholders.md` session, including any routed reopen or follow-up replay paths that previously died on `duplicate leading custom id prefix`.

# Steps

1. Re-run the relevant live `monsterrodholders.md` repair / reopen path against the current `src/agent-doc` branch.
2. Confirm the orphaned-response replay no longer fails on legacy `patch:backlog` plus alias-tag items like `[#ss01]` and `[#wpmem]`.
3. Verify whether the remaining startup-miss warning on pane `%320` clears on the next successful routed cycle or needs a separate route/startup fix.
4. Close or split follow-up work based on the live rerun result.
