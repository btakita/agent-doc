# Plan: Editor selection drives tmux layout automatically

## Invariant

A JetBrains `selectionChanged(newFile)` event is an operator-owned source edge.
If IDEA's `selectedFiles`/split projection still describes the preceding tab,
agent-doc must not publish that stale surface or wait forever for an unrelated
event. The selected-document fact remains authoritative until one current
surface is published or a newer generation supersedes it.

## Reactive ownership

- **Source:** selected document, visible Markdown files, split columns, plugin
  generation.
- **Computed:** whether the visible projection contains the selected document;
  whether a bounded later EDT projection pass remains.
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

## Verification

- JetBrains unit regression for selection authority, bounded settling, and
  non-selection events.
- Source-boundary regression proves the projection path schedules the later EDT
  read without a timer or blocking wait.
- Run JetBrains tests from `make check` through `dev-harness-test`.
