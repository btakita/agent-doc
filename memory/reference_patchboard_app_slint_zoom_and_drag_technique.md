---
name: reference_patchboard_app_slint_zoom_and_drag_technique
description: How patchboard-app does Slint canvas zoom + drag-to-connect (no transform:scale; ancestor TouchArea for wheel; Rust drop hit-test)
metadata: 
  node_type: memory
  type: reference
  originSessionId: 76d13ede-d9a1-4874-8dac-8d3209ce2b77
---

patchboard-app (src/patchboard/patchboard-app) Slint canvas techniques from #spb18:

**Zoom** — Slint has no `transform: scale`. Use *logical zoom*: a `zoom: float`
property + a `pure function zpx(v: float) -> length { return v * 1px * zoom; }`
helper, and multiply every canvas coordinate/size by it (`x: zpx(node.x)`,
`width: zpx(176)`). Fixed-size dots (jacks) stay unscaled but their in-node
*offset* scales so they stay on the edge. Any pointer-delta math (node drag,
drag-preview) must divide by zoom: `(mouse-x - pressed-x)/1px/zoom`.

**Wheel-over-nodes** — a wheel-zoom TouchArea must be an ANCESTOR of the node
elements, not a sibling. Wrap all Flickable content in one TouchArea that only
handles `scroll-event` (returns accept); wheel over a child node bubbles up to
it, while node/jack TouchAreas still handle their own click/drag. A *sibling*
background TouchArea never receives wheel over a node.

**Drag-to-connect** — a TouchArea captures the pointer on press and keeps
`moved`/`pointer-event up` even outside its bounds. On an out-jack: `down` sets
drag props + `click-jack(out)` (sets pending); `moved` (pressed) sets
drag-moved + updates drag-to in canvas coords; `up` calls `drop-jack(x,y)` only
if drag-moved (a no-move tap stays click-to-patch). Drop hit-testing is done in
Rust (`engine::nearest_in_jack`) against projected jack coords within a canvas-px
radius — keeps it testable and dtype-checked; no jack in range → snap-back toast.

See [[project_product_sequencing_patchboard_first_agentdoc_later]]. Plan:
tasks/software/plan-signals-patchboard-full.md; #spb1–#spb20 complete
(#spbwasm iceboxed).
