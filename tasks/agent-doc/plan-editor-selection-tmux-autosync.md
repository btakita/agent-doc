# Plan: Editor selection drives tmux layout automatically

## Invariant

A JetBrains `selectionChanged(newFile)` event is an operator-owned source edge.
If IDEA's `selectedFiles`/split projection still describes the preceding tab,
agent-doc must not publish that stale surface or wait forever for an unrelated
event. The selected-document fact remains authoritative until one current
surface is published or a newer generation supersedes it.

A published surface is positive liveness evidence for its own endpoint. A
lagging `FileEditorManagerEx.windows` snapshot must never make that same publish
retire itself. Endpoint retirement may remove only a previously retained
surface absent from the authoritative live set; an empty backend window list is
unknown liveness for Remote Dev and retires nothing.

## Reactive ownership

- **Source:** selected document, visible Markdown files, split columns, plugin
  generation.
- **Computed:** whether the visible projection contains the selected document;
  whether a bounded later EDT projection pass remains; the authoritative live
  surface set formed from enumerated windows plus the current/published surface.
- **Effect:** publish the current editor surface to the project controller,
  which owns tmux focus/layout reconciliation.

The plugin does not infer tmux state and does not run manual sync. It yields the
EDT between a bounded number of settling projections. The existing generation
guard cancels every stale pass; exhaustion retains the selection for the next
real layout/focus edge.

## Transition table

| State | Observation | Transition |
|---|---|---|
| selected document absent from visible projection | settling pass remains | schedule one later EDT projection |
| selected document visible | generation current | publish surface, clear retained selection |
| newer selection/layout generation exists | any older callback | discard older callback |
| selected document still absent after bound | no pass remains | retain for next real editor edge |
| current/published surface is absent from enumerated windows | backend window evidence exists | add the current surface to the live set; retire only other missing endpoints |
| backend exposes no windows | current surface hint exists | treat liveness as unknown; retire no endpoint |

## Verification

- JetBrains unit regression for selection authority, bounded settling, and
  non-selection events, plus current-surface liveness and Remote Dev unknown
  liveness.
- Source-boundary regression proves the projection path schedules the later EDT
  read without a timer or blocking wait.
- Run JetBrains tests from `make check` through `dev-harness-test`.
