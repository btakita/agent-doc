---
title: Validate direct run template backlog normalization after shared write-path fix
created: 2026-05-01
status: open
---

# Goal

Confirm that `agent-doc run` now completes when the model returns a valid `patch:exchange` plus legacy `patch:backlog` response, instead of failing early on `replace:pending block forbidden`.

# Steps

1. Re-run the direct `agent-doc .../monsterrodholders.md` flow that previously failed after generating `patch:backlog`.
2. Verify the cycle writes the exchange response, normalizes the backlog mutation, and reaches the commit boundary without leaving a pending orphan.
3. Confirm the remaining behavior, if any, is route/startup related rather than template backlog normalization.
4. Close or split any residual live-repro issues into a narrower follow-up.
