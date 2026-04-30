# Plan — clear stale startup-miss markers after a later session start

**Pending:** `#smst`
**Status:** Draft

## Problem

`agent-doc` can leave a per-document startup-miss marker behind for an older pane even after the same document later starts a newer live pane successfully. That makes follow-up checks keep surfacing a stale crash/stranding warning long after ownership has moved on.

Concrete repro from `src/boost-client/tasks/monsterrodholders.md` on April 29, 2026:

- `agent-doc session-check tasks/monsterrodholders.md` prints:
  `startup-miss marker exists for pane %401 (RoutedTrigger) ... latest session_start pane=%408; session log still has no later child exit or session_end`
- `src/boost-client/.agent-doc/state/startup-miss/7b9f2a6394de97c213efeb3ec1e3d33b7b966229655f4720376409f6abef299f.json` still points at pane `%401`
- `src/boost-client/.agent-doc/logs/a9421282-fd96-4943-9af5-3561ed5cb799.log` later records a newer `session_start` for the same document on pane `%408`
- `src/boost-client/.agent-doc/sessions.json` currently registers the document to pane `%408`

So the user-visible outcome still reads like "the tmux pane crashed" even though the real bug is stale startup-miss provenance surviving a later session handoff.

## Desired Behavior

1. A startup-miss marker should only describe the latest relevant ownership attempt for that document.
2. Once a newer pane/session start supersedes the old pane, `session-check` and reroute flows must stop warning about the older pane as if it were the active failure.
3. If a newer pane is itself unhealthy, the warning should point at that newer pane with exact provenance.

## Implementation Steps

- [ ] Reconstruct the stale-marker lifecycle for the `monsterrodholders.md` repro:
  - routed-trigger miss on `%401`
  - later `session_start` on `%408`
  - why no marker clear or supersession happened between those events
- [ ] Audit all marker-clearing paths in `route.rs`, `start.rs`, and `startup_miss.rs`:
  - successful cycle acknowledgment
  - later fresh session starts
  - live-owner recovery / deregistration
  - session-check read path
- [ ] Define the supersession rule explicitly:
  - if the session log shows a later `session_start` for the same document than the marker pane/start, the old marker is stale
  - if `sessions.json` points at a different live pane and the old pane is no longer the registered owner, the old marker is stale
- [ ] Decide where to heal:
  - eager clear during later successful session start
  - lazy clear during `session-check`
  - lazy clear during `route`
  - or a combination with one canonical owner
- [ ] Add provenance logging when a stale marker is cleared as superseded:
  - old pane id
  - newer pane id
  - session id
  - reason the marker was considered stale
- [ ] Add regression coverage for:
  - routed-trigger miss on pane A, later fresh `session_start` on pane B, `session-check` no longer warning on pane A
  - stale marker with no newer owner still warning correctly
  - newer owner exists but is unhealthy, warning pivots to the newer pane/session state rather than the old marker

## Files Likely To Change

- `src/agent-doc/src/startup_miss.rs`
- `src/agent-doc/src/session_check.rs`
- `src/agent-doc/src/route.rs`
- `src/agent-doc/src/start.rs`
- `src/agent-doc/specs/07-commands.md`

## Exit Criteria

- `session-check tasks/monsterrodholders.md` no longer warns about stale pane `%401` once pane `%408` has superseded it
- old startup-miss markers cannot survive silently across later session starts for the same document
- user-facing crash/startup diagnostics always describe the latest owning pane, not historical stale state
