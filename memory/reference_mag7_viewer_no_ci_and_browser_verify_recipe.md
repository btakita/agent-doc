---
name: mag7-viewer-no-ci-and-browser-verify-recipe
description: "mag7-viewer has NO CI workflow (make check is the only gate); brush gestures ARE scriptable via the app's native track-drag listeners; chromium-bridge evaluate quirks"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 7eab6233-9e76-4d7c-a7f8-5442fe0868c5
  modified: 2026-07-23T19:17:09.726Z
---

mag7-viewer facts learned 2026-07-23:

- **No CI workflow exists** (`.github/workflows/` absent; `gh run list --workflow CI` errors). AGENTS.md/CLAUDE.md say to check CI, but local `make check` + real-browser drive are the only gates — same situation as [[reference_efs_repo_has_no_ci_workflow]].
- **Brush verification recipe**: recharts ignores synthetic events, but `TickerCard.tsx` wires its own track/slide drag-select with native `addEventListener` + `clientX` math. Dispatching `new MouseEvent("mousedown", {clientX, clientY, button: 0})` on `.recharts-brush-slide` (allowed at full range), then `mousemove`/`mouseup` on `window`, commits `setRange` for real — no need to force React state. Cross-check footer values against `fetch("/api/returns?start=…&end=…")` sliced by the `.brush-label` dates.
- **chromium-bridge evaluate quirks**: does not await promises (returns `{}` — stash results on `window.__x` and read in a second call); top-level `const` persists across evaluate calls in the same tab (wrap in IIFE); `--tab` pattern matching needs a distinctive substring (port number works).
- Uncommitted WIP may live in the submodule (compoundedReturn experiment in TickerCard/types.ts as of 2026-07-23) — stage selectively via `rtk proxy git diff > patch`, trim hunks, `git apply --cached`.
