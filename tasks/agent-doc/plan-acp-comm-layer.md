# Plan: ACP as agent-doc's communication layer + lazily seqcrdt realtime

**Status:** proposed
**Date:** 2026-07-03
**Owner:** Brian Takita
**De-risking milestone:** [`plan-zed-acp-integration.md`](plan-zed-acp-integration.md) (Zed = first ACP client)

## Summary

Adopt **ACP (Agent Client Protocol)** as agent-doc's *editor-facing* communication
layer across Zed + JetBrains + VSCode, retiring the bespoke cdylib / socket /
file-IPC transport. Keep realtime document semantics in a **separate CRDT layer**
(migrate yrs → **lazily `distributed` seqcrdt**), because ACP is a turn-based
orchestration protocol and structurally cannot express realtime convergence.

**Correction to earlier framing:** ACP does *not* make the CRDT redundant. That
is true only for the weak clobber-avoidance case. Under a **realtime** goal the
CRDT is load-bearing, and ACP is a complementary orchestration veneer, not a
replacement. See [`plan-zed-acp-integration.md`](plan-zed-acp-integration.md)
open-question #3 (superseded by this plan).

## Two layers

| Layer | Protocol | Job | Universal? |
|---|---|---|---|
| Orchestration | ACP | turns, panel, permissions, reviewed edits, streaming diff-preview | yes (Zed/JB/VSCode) |
| Realtime | lazily seqcrdt (or yrs) | concurrent convergence, streaming buffer edits, push subscription | only where we own the client |

ACP does **not** cover the control plane either (supervisor / controller /
routing / claim). That stays as-is regardless.

## Q1 — ACP as the cross-editor editor seam

Editor-side ACP **client** support is now GA, so this is a real decision, not a
bet on the future:

- **JetBrains — first-class, GA.** AI Assistant has native ACP client support in
  IDEs 2025.3+; the JetBrains/Zed **ACP Agent Registry** went live Jan 2026;
  custom agents work **without** a JetBrains AI subscription. `agent-doc --acp`
  registers as a custom agent — no JNA, no cdylib.
- **VSCode — via third-party client extension** (no first-party). Mature options
  exist (e.g. `formulahendry/vscode-acp`, MIT, custom agents). Riskier: you ride
  someone else's client fidelity.
- **Zed — first-party.** Works today.

**Payoff:** one `agent-doc --acp` binary serves all three with zero per-editor
native code; JB+Zed share the registry (register once). Retire the JB cdylib+JNA
plugin and the VSCode koffi plugin.

**Cost / caveats:** it's a *retirement, not a port* — surgical in-buffer
component patching + live highlighting are replaced by ACP's whole-file write +
panel diff review (unless we roll our own client; see below). VSCode fidelity is
someone else's code (verify unsaved-buffer reads). Transports can coexist during
transition (Zed → JetBrains → VSCode, maturity order).

## Roll-your-own ACP client (JB/VSCode only)

JB (full JVM host) and VSCode (`child_process` + buffer API) plugins *can*
implement the ACP **client** role themselves, talking to `agent-doc --acp`. Zed
cannot (WASM sandbox) — Zed is the client. This yields a layered design with
graceful degradation via `initialize` capability negotiation:

1. **Baseline standard ACP (mandatory):** whole-file `fs/write` + permission
   diff. Works in *any* client (Zed first-party, JB first-party, generic VSCode
   extensions). Every editor gets a working agent-doc immediately.
2. **Optional custom-capability clients (JB/VSCode):** negotiate component-granular
   patches, boundary highlighting, and CRDT delta sync. Where present → premium /
   realtime UX; where absent (Zed, generic clients) → transparent fallback to
   baseline.

So we retire the *transport* everywhere, but *may keep* thin JB/VSCode ACP-client
plugins where the surgical/realtime UX earns its maintenance — a per-editor
decision, not all-or-nothing. Zed stays plugin-free by design.

## Realtime investigation (findings)

ACP's full negotiated capability set is closed and contains **nothing** realtime:

- Client: `fs.readTextFile`, `fs.writeTextFile`, `terminal`
- Agent: `loadSession`, `promptCapabilities.{image,audio,embeddedContext}`,
  `mcpCapabilities.{http,sse}`, `auth.logout`,
  `sessionCapabilities.{delete,additionalDirectories}`

Concretely:

- `fs/write_text_file` is **whole-file** (`content` payload) — no delta/patch.
- The `Diff` type in `session/update` is **display-only** (oldText/newText for UI
  render) — a panel preview, not an applied incremental edit.
- No buffer subscription, no CRDT/OT primitive, no agent-initiated mutation
  between turns. Everything is turn-gated behind a client `session/prompt`.

**Zed has the substrate but walls it off:** Zed is itself a CRDT multiplayer
editor (Protobuf RPC, Lamport timestamps, vector clocks), but that system is
**closed — Zed clients only.** Third-party access is an *open feature request*
([zed#50999](https://github.com/zed-industries/zed/discussions/50999)), not a
shipping capability. So the one path to native realtime on Zed is reachable
neither by ACP agents, nor WASM extensions, nor third-party collab clients.

## Realtime tiers + per-editor reality

- **Tier 3 — full realtime:** keystroke-level, bidirectional, chars stream into
  the live buffer, operator edits observed live. Needs a client applying
  incremental CRDT deltas to the buffer.
- **Tier 2 — turn-granular convergence:** operator edits since last read are
  CRDT-merged with agent edits; converged whole-doc written back. No clobber,
  deterministic merge instead of last-write-wins.
- **Tier 1 — raw ACP:** whole-file write, last-write-wins, human resolves diff.

| Editor | Realtime tier | Why |
|---|---|---|
| JetBrains | **Tier 3** | own ACP client + custom CRDT-delta capability |
| VSCode | **Tier 3** | own extension, full buffer API |
| Zed | **Tier 2** (ceiling) | ACP whole-file; extensions sandboxed; collab CRDT closed ([zed#50999](https://github.com/zed-industries/zed/discussions/50999)) |

Custom ACP capabilities cannot lift Zed to Tier 3: the *client* must understand
them, Zed's first-party client won't, and no extension can add it. **Structural,
not a maturity gap.**

## Zed design — ACP + lazily seqcrdt = Tier 2

The seqcrdt lives in the **`agent-doc --acp` agent process** (not a Zed
extension — extensions can't touch buffers). Per turn:

1. `fs/read_text_file(doc)` → current buffer text (incl. unsaved operator edits).
2. **Reconcile snapshot into the seqcrdt:** diff vs last materialization →
   reconstruct operator edits as a concurrent CRDT branch → apply.
3. Apply the agent's response as CRDT ops.
4. seqcrdt converges deterministically → materialize whole document.
5. `fs/write_text_file(converged)` → Zed diffs + applies (per mode/permission).
6. Persist seqcrdt as new base.

Step 2 (opaque text snapshot → CRDT ops, character-identity reconstruction) is
the load-bearing piece. lazily's conformance suite already exercises this shape
(`character CRDT convergence`, `manufactured text identity`, move-aware seqcrdt,
LIS reconciliation) — a genuine fit, **pending verification the reconciliation is
exposed as usable API, not just tested internally.**

**Residual gap (honest):** edits made *during* the read→write window aren't in
the merge; Zed's diff/permission gate surfaces the conflict and re-reading just
before write shrinks the window, but only Tier 3 closes it. No live keystroke
streaming into Zed's buffer. That is the Zed ceiling.

Rejected hacks: polling with frequent auto-turns (chatty whole-file churn, blind
between reads); agent-doc as a Zed terminal thread writing to disk (reintroduces
the disk-clobber problem the CRDT exists to avoid).

## Q2 — lazily as the realtime substrate

agent-doc's tri-state (File / Editor / CRDT) reconciliation *is* a reactive
dependency graph: `Cell(buffer)` / `Cell(disk)` → `Signal(pending_diff)` →
`Effect(emit ACP updates + fs writes)`; permission = a Signal-gated Effect.
lazily is built for exactly this, and its `distributed` seqcrdt (formally proven
convergence, Lean) is the realtime engine.

**One model, many granularities:** the seqcrdt is the shared document model in
agent-doc core; each editor syncs at whatever granularity it can — JB/VSCode
realtime (Tier 3), Zed turn-granular (Tier 2), and Zed graduates to Tier 3 for
free against the same model if [zed#50999](https://github.com/zed-industries/zed/discussions/50999)
ever opens.

Keep lazily a pure reactive/CRDT primitives lib; build the ACP↔CRDT mapping as a
thin adapter (`lazily-acp` sidecar or inside `agent-doc-acp`), not baked into
lazily. **yrs → lazily-seqcrdt is an explicit evaluation, not a foregone
migration** (perf, large-doc behavior, char-CRDT vs component-patch granularity).

## Milestones

- **M0 — Zed baseline (Tier 1→2).** Ship `agent-doc --acp` standard ACP +
  seqcrdt turn-granular convergence. Covered by
  [`plan-zed-acp-integration.md`](plan-zed-acp-integration.md). De-risks the whole
  direction.
- **M1 — JetBrains via ACP (Tier 3).** Roll-your-own ACP client with custom
  CRDT-delta capability; retire the cdylib+JNA transport.
- **M2 — VSCode via ACP (Tier 3).** Own client or vetted third-party; retire the
  koffi transport.
- **M3 — yrs → lazily-seqcrdt** (if the evaluation clears).

## Open verification items

1. **lazily snapshot→ops reconciliation API** exposed and usable (not just
   internal test fixtures)?
2. **ACP `fs/write` — live buffer or disk?** (explicit for read = unsaved buffer;
   ambiguous for write). Verify via `dev: open acp logs`.
3. **VSCode third-party client fidelity:** does its `fs/read` return unsaved
   buffer; diff/permission rendering quality?
4. **Custom ACP capability negotiation** — confirm generic clients degrade
   gracefully (ignore unknown capabilities).
5. **yrs → lazily-seqcrdt** perf/large-doc/granularity evaluation.
6. **Watch [zed#50999](https://github.com/zed-industries/zed/discussions/50999)** —
   third-party collab protocol = Zed Tier 3 path.
7. **Turn triggering** for document-driven (mode-A) turns (client-initiated turn
   constraint).

## References

- ACP: [schema](https://agentclientprotocol.com/protocol/schema) ·
  [initialization](https://agentclientprotocol.com/protocol/initialization) ·
  [file system](https://agentclientprotocol.com/protocol/file-system) ·
  [prompt turn](https://agentclientprotocol.com/protocol/prompt-turn)
- JetBrains: [ACP](https://www.jetbrains.com/acp/) ·
  [ACP docs](https://www.jetbrains.com/help/ai-assistant/acp.html) ·
  [ACP Registry (Jan 2026)](https://blog.jetbrains.com/ai/2026/01/acp-agent-registry/)
- Zed: [ACP](https://zed.dev/acp) · [Channels/Collab](https://zed.dev/blog/channels) ·
  [zed#50999 cross-editor collab request](https://github.com/zed-industries/zed/discussions/50999)
- VSCode: [formulahendry/vscode-acp](https://github.com/formulahendry/vscode-acp)
- Internal: lazily `distributed` seqcrdt (`src/lazily-rs`, `seqcrdt_conformance`),
  agent-doc `editors/PLUGIN-SPEC.md` (tri-state model)
