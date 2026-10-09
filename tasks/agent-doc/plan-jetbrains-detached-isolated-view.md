# Plan — JetBrains Remote Dev detached isolated terminal view (GH #218)

Status: implementation may open a post-release PR, but must not merge before the
operator upgrades backend and Client/Gateway to IDEA 2026.2.3 and confirms the
two-sided plugin distribution path.

This plan supersedes the ownership semantics in
`plan-jetbrains-detached-terminal-surface.md` for GH #218. That earlier plan and
PR #219 supplied local surface/tool-window substrate; they do not satisfy this
feature's main-window isolation contract.

## Platform and packaging decision

Ship two independently verifiable JetBrains artifacts:

1. The existing 242 classic artifact remains source- and behavior-compatible.
   On Remote Dev it remains fail-closed when detached-frame identity is not
   available; it never guesses a frame from focus, open order, or a flat editor
   set.
2. A new 262 split artifact contains frontend, shared, and backend modules. The
   frontend captures frame-tagged editor evidence before dedupe, the shared
   module owns serializable RPC DTOs, and the backend authenticates the client
   session and publishes typed facts into the controller graph.

The split artifact is installed on both backend and JetBrains Client/Gateway.
Development may use explicit two-sided installation. Production distribution
must use Marketplace or a custom plugin repository because a local ZIP on one
side is not synchronized to the other side.

## Architecture contract

**Invariant:** A detached-frame event cannot add, remove, reorder, resize,
select, focus, join, break, swap, or stash a pane in the main `agent-doc`
window. A document visible in any main-frame split is main-owned; every
detached occurrence renders a placeholder and owns no pane.

**Policy owner:** `EditorViewPolicy` in `agent-doc-editor-surface` is the sole
owner of main precedence, detached ownership, placeholder decisions, stale
evidence rejection, and lifecycle transitions. `MainLayoutEligibility`,
derived from its durable bindings, is the sole controller/tmux exclusion
projection. JetBrains and tmux layers observe facts and execute typed effects;
they do not choose ownership.

**Transition table:**

| Current state | Authenticated evidence/event | Decision/effect | Next durable state |
|---|---|---|---|
| `Released` | detached-only document in one surface | append bind intent; create isolated session; move exact stashed pane; verify | `Bound` |
| `Released` | same document in main and detached | main owns; detached placeholder; no tmux mutation | `Released` |
| `Bound` | same document becomes main-visible | append release intent; move exact pane to main stash without selecting/reflowing main; verify; remove view session; permit later main publication | `Released` |
| `Bound` | detached focus/sync/route/resize | target only the view session/widget | `Bound` |
| `Bound` | owning detached frame closes | append release intent; move exact pane to main stash while preserving main current window/active pane; verify; remove view session | `Released` |
| `BindPending` / `ReleasePending` | backend restart or reconnect | hydrate before first publication; survey tmux; finish or compensate idempotently | `Bound` / `Released` |
| any | stale sequence, old frontend epoch, old client generation, or retired frame | reject; no IDE/tmux effect | unchanged |
| any | incomplete snapshot, zero or multiple main roles, unavailable RPC, or no terminal surface | fail closed; freeze main publication and show diagnostic/placeholder | unchanged |
| `Bound` | second detached surface shows same detached-only document | retain deterministic durable owner; other detached surfaces show placeholder | `Bound` |
| any | two thin clients use the same pane id | isolate by backend-stamped `(ClientId, connection_generation, pane_id)` | per-client state unchanged |

`BindPending`, `Bound`, and `ReleasePending` all exclude a document from main
sync. Only a verified `Released` receipt makes it eligible again. A detached
lifecycle acquires panes only from main stash; it never removes a pane directly
from the main `agent-doc` window.

**Evidence inputs:**

- Frontend snapshot: frontend instance/epoch, monotonic sequence, project id,
  completeness, pane id, role (`root` = main), focus, and per-`EditorWindow`
  selected/open/visible paths captured before dedupe.
- Backend-authenticated facts: `ClientId`, connection generation, client-session
  disposal, project identity, and RPC freshness/error.
- Durable binding: document hash/path, binding epoch, view id, client family,
  surface id/generation, isolated session, pending/settled state, pane receipt,
  release reason/destination.
- Tmux receipt: exact pane id/session/window, session/window geometry, main
  current window, and main active pane before and after an effect.
- JetBrains receipt: widget/tool-window identity, target pane id, placeholder
  state, and capability failure.

Frontend DTOs never supply authoritative client identity. The backend stamps
`ClientId` and connection generation from the RPC/session boundary. Sources are
keyed by `(project, client, connection_generation, pane_id)`; a complete first
snapshot gates publication, old sequences/generations are rejected, omitted
panes retire only on a complete snapshot, and the generation retires on client
session disposal.

**Reactive topology:**

```text
JetBrains Client frame/editor/docking events
  -> frontend complete SurfaceSnapshot Source
  -> durable typed RPC stream
  -> authenticated/generation-fenced BackendSurfaceSnapshot Source
  -> EditorViewPolicy Computed
     -> main-frame columns/focus Computed
     -> detached ownership/placeholder Computed
     -> MainLayoutEligibility Computed
     -> durable binding transition Computed
  -> append durable intent Effect
  -> tmux + JetBrains idempotent Effects
  -> verified effect-receipt Sources
  -> durable settled-state Effect
```

State hydration and pending-effect reconciliation gate the first main layout
publication. Ingress is event-driven; there is no steady-state poller. A
one-shot tmux survey is permitted only during restart reconciliation because
tmux exposes no replayable lifecycle event history.

**Imperative extraction audit:**

- Replace project-global `RemoteLayoutMemory` path state with a workspace
  resolution keyed by authenticated surface identity. Per-surface evidence is
  never folded through a project-global mutable cache.
- Replace each caller's view-bound filtering with one retained
  `MainLayoutEligibility` computed projection.
- Replace the focused-surface-wins duplicate rule in `SurfaceTerminalOwnership`
  with main precedence plus deterministic detached ownership.
- Replace direct detached calls to shared `focus_document_pane` /
  `sync_tmux_layout` with a view-target decision/effect.
- Replace singleton terminal lookup with a per-surface host projection and
  explicit placeholder state.
- Keep frontend event capture, RPC send, tmux command execution, JetBrains
  widget mounting, and restart tmux survey as boundary effects. None owns
  derived policy.

## Durable state and tmux lifecycle

Use one superseding state fact per document:

```text
EditorViewBindingObserved {
  document_hash, canonical_path, binding_epoch,
  state: BindPending | Bound | ReleasePending | Released
}
```

Every nested state contains sufficient reconstruction metadata. Add
`StateDomain::EditorView`, fold the newest binding into
`DocumentStateProjection`, retain one newest fact per document, hydrate it into
the controller graph before accepting editor observations, and fence settle or
release receipts by binding epoch plus view id.

Each detached owner uses a separate session named from a bounded hash of
`(project, client, surface)` and a window named `view` (never `agent-doc` or
stash-like). Binding is intent -> create placeholder session -> explicitly
authorized cross-session move of the exact pane -> remove placeholder -> verify
same pane id and exactly one pane -> settle receipt. Release is intent -> move
the exact pane to main stash while preserving main selection/focus -> verify ->
delete view session -> settle receipt.

Ordinary tmux repair/sync must treat view sessions and panes as protected.
Cross-session join/swap/break remains forbidden unless the command carries the
dedicated editor-view lifecycle authorization. Update registry and durable actor
window/session placement receipts after every move while preserving document,
pane id, actor generation, and supervisor identity.

`MainLayoutEligibility` must be consumed before:

- main sync column construction and pane candidate resolution;
- route merge and retained-layout replacement;
- focus escalation and pane promotion;
- observed-pane/width bounds;
- remote layout `column_order` replacement accounting;
- reverse tmux-to-editor focus;
- resync wrong-session detection/fix;
- start-time wrong-session relocation;
- tmux-router spare assignment, swap, join, break, detach, and reorder.

## Allowed edit surfaces

- New JetBrains 262 split artifact projects: root packaging, frontend observer,
  shared DTO/RPC, backend RPC ingress, split-mode test configuration.
- Existing JetBrains 242 artifact only for explicit fail-closed compatibility
  tests or shared packaging metadata; no source behavior regression.
- `agent-doc-editor-surface` and `agent-doc-editor-surface-io` for pure policy,
  tagged remote layout, and authenticated wire adaptation.
- `agent-doc-state-backbone` / SQLite projection for durable binding facts.
- `agent-doc-controller-io`, `agent-doc-sync-io`, `agent-doc-start-io`, focus and
  reverse-focus adapters for the single main-eligibility projection.
- `tmux-router` only if its generic session-scope guard must learn protected
  pane/session evidence; otherwise keep the authorization adapter in agent-doc.
- Focused specs, deterministic policy/SimWorld tests, and `IsolatedTmux`
  integration fixtures.

No version/tag/release/publish/global-install change and no live session-document
edit is allowed in this implementation PR.

## Verification

1. Package/build both artifacts independently. Plugin Verifier proves the 242
   classic artifact remains compatible and the 262 artifact has loadable
   frontend/shared/backend modules. Inspect each ZIP/module descriptor.
2. A split-mode test target launches frontend and backend, proves typed RPC
   discovery, complete frame-tagged snapshots before dedupe, `root` main role,
   detached frame lifecycle, and fail-closed transport loss.
3. Pure policy tests cover every transition row, including main precedence,
   two detached duplicates, stale epochs, reconnect, two clients with identical
   pane ids, no surface, and no terminal capability.
4. Controller tests prove view-bound documents cannot enter sync, route merge,
   focus escalation, width bounds, `column_order`, or reverse focus.
5. State/SQLite tests prove binding persistence, newest-fact retention, hydration
   before first publication, pending-effect replay, and stale receipt fencing.
6. `IsolatedTmux` tests snapshot main panes/stash/current window/active pane and
   assert byte identity across detached open/focus/route/sync/close/restart.
   Reject forbidden ordinary cross-session join/swap/break. Run geometry matrix
   `latest|largest|smallest` x aggressive-resize on/off with 240x50 and 100x30
   clients and assert no main geometry/focus contention.
7. Run focused crate/Gradle checks, `make tmux-ci`, `make check`, private-name
   hygiene, fresh CI, and current-base readiness. Open a PR but do not merge.

## Out of scope

- A read-only terminal mirror (`pipe-pane`/capture loop); detached duplicates use
  placeholders.
- Supporting the split feature through 242 internal Fleet RPC.
- Automatically copying a local ZIP from Linux backend to a Windows client.
- General multi-terminal UX or moving unrelated stock Terminal tabs.
- Merging before the operator confirms the 262 backend/Client/Gateway upgrade
  and two-sided distribution path.
