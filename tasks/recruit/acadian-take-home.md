---
agent_doc_session: 7605b2c4-eaba-4a4c-9739-bb52a499521e
resume: ad1125c2-24f9-4650-942c-4b774833fed2
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
queue: stop
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Re: Acadian take-home implementation plan — fable-5

Read `tasks/recruit/acadian/acadian-take-home.md` — the assignment is a **MAG7 Interactive Return Viewer**: FastAPI backend (`/returns?start=&end=`) computing daily % returns for MSFT/AAPL/GOOGL/AMZN/NVDA/META/TSLA via yfinance with in-memory caching, and a React 18 frontend with a responsive per-ticker grid of interactive line charts (tooltip/zoom, min/max/mean), a date-range picker that re-fetches, plus bonus items (summary table, graceful errors, modular typed backend, tests).

Plan written to **`tasks/recruit/acadian/plan.md`**. Summary:

- **New standalone repo `mag7-returns`** (submission needs a public GitHub repo; keeps private-workspace names out per private-name-hygiene runbook). `backend/` (uv + FastAPI) and `frontend/` (Vite + React 18 + TS + Recharts).
- **Backend modularity** targets the mid-level rubric: `fetcher.py` (one batched `yf.download` for all 7 tickers), `returns.py` (pandas `pct_change`, typed), `cache.py` (TTL keyed by `(start, end)`), `api.py` (validation → 422/400, yfinance failure → 502 with clear message), pytest suite with mocked yfinance so tests need no network.
- **Frontend**: CSS-grid `auto-fit` layout, Recharts `LineChart` + built-in tooltip + `Brush` for zoom, native date inputs with debounce + AbortController re-fetch, client-side min/max/mean in a unit-testable `lib/stats.ts`, `SummaryTable` with cumulative return across all 7 names, error banner + loading skeletons.
- **Assumptions to document**: returns on adjusted close, first day of range dropped (no prior close), trading days only, TTL cache is the rate-limit mitigation.

Queued as 5 phase items **#acad1–#acad5** (scaffold → backend → frontend → bonus polish → submission), each pointing at the plan file. Say `do #acad1` (or `go`) to start implementation.
Please read tasks/recruit/acadian/acadian-take-home.md

Please create a plan to implement.

### Re: private repo + submodule — fable-5

> **Queue prompt:** Create a private gh repo btakita/acadian-take-home. Add as a submodule in src/acadian-take-home

Done.

- Created **`btakita/acadian-take-home`** (private) via `gh repo create`.
- Seeded it with an initial commit (stub README naming the `backend/` + `frontend/` split) and pushed `main`, so it was a valid submodule target.
- Added it at **`src/acadian-take-home`** — registered in `.gitmodules` as `submodule.src/acadian-take-home`, cloned and staged.
- Committed the superproject: `228a8917 acadian-take-home: add private repo as submodule at src/acadian-take-home`.

Note: the repo name is `acadian-take-home`, not the `mag7-returns` name the plan file proposed — I followed your queue item. `tasks/recruit/acadian/plan.md` is gitignored under this workspace, so it stays untracked here; the plan content moves into the submodule's own README during the scaffold phase.

Next queue item is `/goal do tasks/recruit/acadian/plan.md`, which drains #acad1–#acad5 into the new submodule.

### Re: MAG7 return viewer implemented — fable-5

> **Queue prompt:** /goal do tasks/recruit/acadian/plan.md

Built, verified end-to-end, and pushed to **`btakita/acadian-take-home`** (commit `a9016be`). Superproject gitlink updated at `d10e9acb`. All five phases (#acad1–#acad5) are done.

**Backend** (`backend/`, FastAPI + pandas + yfinance, fully typed):

- `fetcher.py` does one batched `yf.download` for all 7 tickers and normalizes both the MultiIndex (multi-symbol) and flat (single-symbol) response shapes; `returns.py` holds the pure `pct_change` transform; `cache.py` is a generic thread-safe TTL+LRU cache; `service.py` composes them; `api.py` owns validation and error mapping.
- Errors map deliberately: 422 malformed dates, 400 inverted or over-long range, 502 upstream failure or a range too thin to difference — each with a message written for a human.
- Both the fetcher and the service are injectable, so **29 tests run with no network**. `mypy` passes with `disallow_untyped_defs`.

**Frontend** (`frontend/`, React 18 + TS + Recharts):

- CSS-grid `auto-fit` layout, one card per ticker: line chart, tooltip, `Brush` zoom handles, min/max/mean footer, cumulative return in the header.
- Date changes trigger a debounced, abortable re-fetch, so a slow earlier response can't overwrite the range you're looking at.
- Sortable cross-ticker summary table (days, mean, min, max, std dev, cumulative) and an error banner with retry. 9 unit tests on the stats helpers; cumulative return compounds rather than sums.

**Live verification**, not just green tests — ran both servers and drove the real app in Chromium:

- All 7 cards rendered with real yfinance data and no errors; the screenshot shows charts, stats, and the summary table.
- Changing the date range re-fetched correctly (June 2026 → 20 trading days).
- Killed the backend and confirmed the error banner plus Retry appeared; restarting and clicking Retry recovered to 7 cards. That test exposed a real gap — the dev proxy turns a downed backend into a bodiless 500, which showed as a bare status code — so 5xx-without-detail now reads "The server is unavailable. Is the backend running?"

`make check` is green across the board: 29 backend tests, mypy clean, 9 frontend tests, typecheck, production build.

Two notes worth your attention. The repo is **private**, so the reviewer will need an invite or a zip before submission. And the plan file proposed the name `mag7-returns`, but I used `acadian-take-home` per your queue item — the app title inside is still "MAG7 Return Viewer".
Add a dev uv task to start the dev server.
commit + push

### Re: uv dev task, committed and pushed — fable-5

Added and verified. Pushed as `9c492fd`; superproject gitlink at `0d744741`.

`uv` 0.11.28 has no `[tool.uv.tasks]` table — I probed it and it just tries to spawn the task name as a binary — so the idiomatic equivalent is a `[project.scripts]` entry point:

```bash
cd backend && uv run dev
```

`app/dev.py` exposes `main()`, registered as `dev = "app.dev:main"`. It starts uvicorn with autoreload and takes `--host`, `--port`, and `--no-reload`, reading `HOST`/`PORT` from the environment as defaults. Verified live: `uv run dev --port 8010` came up with the reloader active and `/health` answered `{"status":"ok"}`.

`make dev-backend` now delegates to `uv run dev` instead of spelling out the uvicorn invocation, and the README documents the task and its flags.

Two things worth flagging. `uv run` generated `backend/uv.lock` (1306 lines) and I committed it — for an application that's the right call, since it pins the resolved dependency set for whoever clones the repo, but say the word if you'd rather it stay untracked. And `make check` is green after the change: 29 backend tests, mypy clean across 9 source files now, 9 frontend tests, typecheck, production build.
❯ Please allow me to drag a range on the graph itself. Ensure the sliders are synced. commit + push

### Re: drag-to-zoom on the chart, synced sliders — fable-5

> **Queue prompt:** Please allow me to drag a range on the graph itself. Ensure the sliders are synced. commit + push

Done. Pushed as `156659e`; superproject gitlink at `48c2fd6e`.

You can now press and drag anywhere across a chart's plot area to select a range. A shaded band previews the selection while you drag, and releasing zooms to it. A **Reset zoom** control appears in the card header once a chart is zoomed.

**How the sliders stay synced.** The chart and its brush are driven by a single controlled index range: dragging on the plot writes that state, and the brush's `startIndex`/`endIndex` read from it, so the handles follow. The brush's `onChange` writes back to the same state, so dragging the handles rezooms the plot. One source of truth, both directions.

One subtlety worth naming: the selection resolves through x-axis **labels**, not the chart's own point indices. Recharts reports indices relative to whatever window the brush currently shows, so index math would drift once you were already zoomed in — dates are unique across the whole series and don't have that problem. Right-to-left drags normalize to the same range, and a click without movement is ignored rather than collapsing the view.

**Verified with real browser input**, not synthetic events — my first attempt with dispatched `MouseEvent`s silently failed to reach the brush's document-level listeners, so I drove actual CDP mouse events instead:

- Dragged across the plot from x=372 to x=462: the chart narrowed from Mar 11–Jul 17 to Mar 12–Apr 29 and the slider handles landed at 371/458.
- Dragged the left brush handle from 375 to 330: the chart widened back to Feb 19, confirming the reverse direction.
- Reset zoom restored the full series, and the other six cards were unaffected — zoom is per-card.

The screenshot also caught a flaw worth fixing: dragging was highlighting the axis labels as selected text. Added `user-select: none` on the chart container and confirmed the selection stays empty through a drag.

`make check` is green: 29 backend tests, mypy clean, 22 frontend tests (13 new ones covering the range math — ordering, click rejection, unknown labels, and clamping stale indices), typecheck, production build.
The Loading... should not displace the grid. Use a spinner instead that is discretely placed at the boundary of the view. Use UI best practice for a stock analytics dashboard.

### Re: ambient loading spinner — opus-4-8

Fixed. The `Loading returns…` paragraph was a block element sitting in the document flow directly above the grid, so every fetch pushed the charts down and then snapped them back. Two changes remove the displacement:

**First load** now renders a `SkeletonGrid` — seven placeholder cards mirroring the real card box model (header row, 180px plot, three-up stats footer). The grid occupies its final geometry before data arrives, so cards swap in without a reflow.

**Refetches** keep the existing grid mounted and visible (stale-while-revalidate). The `.results` wrapper gets `aria-busy` and dims to `opacity: 0.65`; the data you were reading stays exactly where it was. That is the pattern trading dashboards use — a range change should never yank the chart you are mid-read on.

**The indicator itself** is a `position: fixed` pill at the bottom-right boundary of the viewport: a 14px spinner plus a label that reads `Loading returns…` on first paint and `Updating returns…` on a refresh. It is `pointer-events: none`, carries `role="status"` + `aria-live="polite"` so screen readers get the announcement the removed paragraph used to provide, and honours `prefers-reduced-motion` (skeleton pulse off, spinner slowed).

The render decision lives in `lib/viewState.ts` as two pure functions — `viewState()` returns `skeleton | grid | empty | blank` and `isRefreshing()` distinguishes a first load from a refresh — so it is unit-testable without a DOM harness. Six new tests cover it.

Verified live in Chromium against the running dev servers, measuring `.grid` top offset across all three states:

| state | grid top | cards | spinner | opacity |
|---|---|---|---|---|
| first-load skeleton | 95.296875 | 7 | yes, `Loading returns…` | 1 |
| mid-refresh | 95.296875 | 7 | yes, `Updating returns…` | 0.65 |
| idle with data | 95.296875 | 7 | none | 1 |

Identical to the fractional pixel — zero displacement. `aria-busy` flips `true`/`false` correctly, and the spinner computes to `position: fixed`.

`make check` is green: 29 backend tests, mypy clean, 28 frontend tests (6 new), typecheck, production build.

Pushed as `dcaa862` on `btakita/acadian-take-home`, superproject gitlink at `31e33b562`. Also untracked `frontend/tsconfig.tsbuildinfo` (`a431a7e`) — a build cache that was dirtying the tree on every typecheck.

One unrelated observation worth naming: on the default trailing-6-month range the backend returned only six symbols — MSFT was absent — while a shorter range returned all seven. The API drops a requested ticker silently rather than reporting it as unavailable. That is a backend data-contract gap, not a UI one; I have left it alone and queued it as a backlog item.
❯ When I change the sorting in the Summary table, the columns resize. Please ensure consistent column widths.
### Re: stable summary table column widths — opus-4-8

Fixed, and the root cause turned out to be narrower than I first assumed.

**What was actually moving:** the `↓`/`↑` glyph was appended only to the *active* header. Sorting by a different column removed the glyph from one header and added it to another, changing both headers' intrinsic widths — and under the browser's default `table-layout: auto`, that re-solves the layout for *every* column. Row reordering was innocent; the arrow was the whole story.

I confirmed this rather than assuming it. Injecting the old unreserved-arrow behaviour back into the live page produced **5 distinct column layouts across 7 sort keys**. Restoring the reserved slot while leaving auto layout in place was already stable — so the arrow was necessary and sufficient to cause the jump.

**The fix, two layers:**

- The arrow now renders in a fixed-width `.sort-arrow` slot present in *every* header regardless of sort state (`aria-hidden`, since `aria-sort` already carries the semantics). This addresses the actual cause.
- `table-layout: fixed` with an explicit `<colgroup>` declares the geometry outright instead of deriving it from cell content. This is the belt-and-braces guarantee — it makes the widths independent of the data too, so a longer ticker symbol or a four-digit day count can't shift things either. `min-width: 680px` keeps columns legible on narrow viewports, with `.summary-scroll` taking the overflow as before.

**Verified live** across all 7 columns × both directions, with each render awaited (my first probe read the DOM synchronously after `click()` and was measuring pre-render state — worth flagging, since that probe would have reported a false pass):

| | result |
|---|---|
| samples | 14 (7 columns × 2 directions) |
| column-width mismatches vs baseline | 0 |
| distinct row orders observed | 13 |
| `aria-sort` tracking | correct, single active column |

Widths held at `[190.117, 135.797, 190.117, 190.117, 190.117, 190.117, 271.617]` in every sample — identical to the fractional pixel — while 13 distinct row orders prove the sort is genuinely applying rather than the table having gone inert.

`make check` is green: 29 backend tests, mypy clean, 32 frontend tests (4 new pinning the column-width contract), typecheck, production build.

Pushed as `50ded3a` on `btakita/acadian-take-home`; superproject gitlink at `45ee7d6da`.

### Re: unavailable symbols, anchored tooltip, themed brush — opus-4-8

> **Queue prompt:** do [#backendsilentlydrops]
> **Queue prompt:** The tooltips are "dancing" around when I move the mouse over the line graph. Please keep the tooltips anchored to the mouse and with a fixed reference point (no dancing). Only change the position in relation to the mouse when the viewport would otherwise cut it off.
> **Queue prompt:** The selection slidebar background is not pleasing. Please use ui best practices to ensure the color fits well with the background.

All three done. Note the queue was not in `go` mode — I read three concrete work items plus an explicit invocation as the instruction and drained them; say so if you would rather I wait for `go` next time.

**1. Unavailable symbols are now reported, not omitted.** The cause was `dropna(axis=1, how="all")` in `_extract_close` combined with a `present` filter: a ticker with no usable prices simply vanished from the response. `GET /returns` now answers `{data, unavailable}`, where every requested symbol appears in exactly one of the two and each unavailable entry carries a reason. The two causes are distinguished — no prices at all, versus prices with no complete trading day to difference. The frontend names them in a notice above the grid, grouped by reason (`groupByReason`), because a response can carry mixed causes and rendering one symbol's reason for the whole list would misreport the others.

**2. The tooltip is anchored.** Three separate things were moving it, and the first two only showed up under measurement:

- Recharts derives placement from the *active data point*, not the cursor.
- The wrapper carries `transition: all`, so the panel glides after the pointer. `isAnimationActive={false}` does **not** remove this — it needed a CSS override.
- The panel sizes to its own content, so the cursor-to-panel gap changed with the value being displayed.

It now sits a constant 14px from the cursor at a fixed `y`, at a fixed width, flipping sides only when the right edge would clip it. `anchorX()` holds that as a pure function with 6 unit tests.

I also had a wrong constant: I estimated the panel at 148px when it renders at 118px, which made it flip ~47px early and left a visible floating gap. Measurement caught it; the width is now pinned in CSS and the constant documents that the two must stay in sync.

**3. The brush is themed.** Recharts hardcodes white/grey presentation attributes, which glared against the dark card. CSS overrides them (CSS beats presentation attributes). My first attempt had a selector bug — `rect:first-of-type` also matched the slide, so track and selected window resolved to the *same* color and the selection became invisible. Caught it by reading computed fills rather than eyeballing the screenshot. The track is now `--grid`, the selected window `--axis` at 0.35, handles `--muted` with a surface-colored stroke.

**Live verification**, continuous 2px pointer sweep across the plot area:

| | result |
|---|---|
| tooltip `y` | constant (6) across all samples |
| tooltip width | constant (118) across all samples |
| cursor-to-panel gap | constant 14px |
| edge flips | exactly 1 |
| wrapper transition | `none` |

Three early samples in an initial sweep looked anomalous; they were at x = 10/20/30, and the plot area starts at x = 40 — outside it there is no data point and no tooltip to track. Every in-plot sample was constant. I also confirmed the notice with a synthetic mixed-cause payload: `MSFT, TSLA` grouped under one reason, `META` under another, four cards remaining.

`make check` is green: 33 backend tests (4 new), mypy clean, 42 frontend tests (10 new), typecheck, production build. README updated for the new payload shape.

Pushed as `f7f4fa3` + `2a26971` on `btakita/acadian-take-home`; superproject gitlink at `662c1fec1`.

### Re: reset zoom as an icon beside the slider — opus-4-8

> **Queue prompt:** Change the "Reset Zoom" button text into a reset icon. Move the Reset Zoom button to the right of the slider. commit + push

Done. The control is now a 20px icon button (circular-arrow glyph) sitting in the gutter immediately right of the brush, and the card header is back to just the symbol and the cumulative return.

Two details worth naming:

**The gutter is reserved unconditionally.** `RESET_GUTTER` is applied to the chart's right margin whether or not the button is showing, and the button itself is absolutely positioned. If the space were only reserved while zoomed, the plot and brush would resize the moment you zoomed — the same class of displacement you flagged with the loading indicator.

**The label survives as an accessible name.** Icon-only buttons routinely lose theirs. `Reset zoom` is still there via an `.sr-only` span (verified computed at 1×1px, clipped) plus a `title` attribute for hover.

**Verification.** Synthetic mouse events do not drive Recharts' drag, so I could not zoom the chart programmatically. I force-rendered the control to measure its geometry, then reverted that and confirmed the gate was back — worth stating plainly, since a measurement taken under a temporary edit is only as good as the revert:

| check | result |
|---|---|
| in chart, not header | `inChart: true`, `inHeader: false` |
| right of the brush | button spans 313.5–333.5; brush ends at 307.5 |
| vertical alignment | centers within 1.9px of the brush band |
| icon, not text | `<svg>` present, label visually hidden at 1×1px |

Then one of my earlier brush drags turned out to have actually landed, leaving AAPL genuinely zoomed — so the real end-to-end test was available after all. The button appeared on that card only, and clicking it restored the axis from `2026-03-17 … 2026-05-29` back to the full `2026-02-03 … 2026-07-20` range, with the button disappearing on reset.

`make check` is green: 33 backend tests, mypy clean, 42 frontend tests, typecheck, production build.

Pushed as `3be8199` on `btakita/acadian-take-home`; superproject gitlink at `c90969813`.

One unrelated note: the frontmatter reads `agent: c`, which preflight flags as a harness mismatch on every cycle (it falls back to the active harness, so nothing breaks). Likely a truncated `claude` — worth fixing to silence the warning.

### Re: reset button unclickable, crosshair cursor — opus-4-8

You were right, and my verification had a hole that let it through.

**Cause.** Recharts' own wrapper is `position: relative` and comes *after* the button in the DOM, so it painted over the absolutely-positioned button. The chart surface took the hit instead — which both swallowed the click and showed its `crosshair` cursor. Fixed with a stacking order (`z-index: 3`) and an explicit `cursor: pointer`.

**Why my check missed it.** I verified the button with `btn.click()`, which dispatches directly on the element and bypasses hit-testing entirely. It passes just as happily on an element buried under an overlay. The click I "confirmed" last cycle never went through the pointer path a real user takes, so the result was meaningless for this failure mode.

**Re-verified properly**, using the pointer path rather than the element:

| check | before | after |
|---|---|---|
| topmost element at button centre | chart surface | the button's own `<svg>` |
| computed cursor at that point | `crosshair` | `pointer` |
| event dispatched at the centre coordinate | ignored | resets the zoom |

The last row is the real test: I resolved the element via `document.elementFromPoint` at the button's centre and dispatched there, and AAPL went from its zoomed window `2026-02-02 … 2026-06-11` back to the full `2026-02-03 … 2026-07-20`, with the button disappearing.

The general lesson I am taking from this: `element.click()` proves a handler is wired, never that a control is reachable. For anything positioned or overlaid, the check has to go through `elementFromPoint`.

`make check` is green: 33 backend tests, mypy clean, 42 frontend tests, typecheck, production build.

Pushed as `9eb3d0a` on `btakita/acadian-take-home`; superproject gitlink at `c794c0205`.

Your next queue item — moving the date tooltips below the selector and keeping them legible and non-overlapping when the handles are close — is still queued and unstruck; I will take it on the next invocation.

### Re: brush date labels, and mouse-driven zoom reset — opus-4-8

> **Queue prompt:** Move the date selection tooltip below the selector. Make sure the tooltip is always legible and does not obscure other UI data. Also handle when the two sliders are close together. Make sure the date tooltips do not overlap.

Done. Recharts draws those labels inline on the brush row, which is why they crowded the handles and ran under the reset control in your screenshot. They are hidden now and re-rendered in a row of their own below the brush.

`placeBrushLabels()` owns the geometry as a pure function: centre each label under its handle, clamp inside the container, and — when the handles are close enough that the labels would collide — push them apart symmetrically about their midpoint and re-clamp, preserving *separation* over centring. If the container cannot fit both side by side, they pin to the two edges, which is the most legible degradation available. Handle positions are measured from the rendered SVG in a layout effect rather than derived from indices, so they stay correct regardless of the chart's internal margins.

**Verification.** Recharts' brush does not respond to synthetic pointer events, so I forced narrow windows in code, measured, then reverted and confirmed the revert:

| handle gap | position | label gap | overlap | in container |
|---|---|---|---|---|
| 4.2px | mid-chart | 6px | none | yes |
| 4.3px | hard left | 6px | none | yes, `left inset 0` |
| 4.2px | hard right | 6px | none | yes, `right inset 0` |
| full range | — | 181.6px | none | yes |

The unit tests also sweep every handle-position pair across the container and assert no overlap and no out-of-bounds, which is the part that generalises beyond the four cases I could stage by hand.

> **Queue prompt:** What is UX best practice to support resetting the zoom from the mouse? Don't implement anything, just discuss.

Discussion only — nothing implemented. Four conventions, roughly in order of how strongly I would argue for them here:

**1. Double-click the plot to reset.** This is the closest thing to a standard in charting — Plotly, Highcharts, and TradingView all use it, and it is the one gesture users are most likely to try unprompted. It costs nothing in screen space, it is discoverable by accident, and it is safely reversible. The one caveat is that it must not fight the drag-to-zoom selection: the handler has to ignore the double-click if a drag actually moved, or a sloppy fast drag reads as a double-click.

**2. Right-click / context menu to zoom out one step.** Common in desktop finance terminals. I would not reach for it in a browser app — overriding the browser context menu is a real cost, and web users do not expect it.

**3. Escape key.** Not strictly a mouse gesture, but the natural keyboard partner to any of these, and cheap. Worth having regardless of what the mouse does, since it also serves keyboard-only users who cannot reach the icon without tabbing.

**4. Scroll-wheel zoom, with reset as "scroll all the way out."** This is the most fluid model and the one people know from maps. It is also the most invasive: wheel-over-chart hijacks page scrolling, which is actively hostile on a page of seven stacked cards like this one. If it were ever added it should require a modifier (Ctrl/Cmd + wheel) so a plain scroll still moves the page.

My recommendation for this dashboard specifically: **double-click on the plot, plus Escape, keeping the icon button.** The double-click covers the users who expect a gesture, Escape covers keyboard users, and the button stays because it is the only affordance that is *visible* — gestures are undiscoverable on their own, which is exactly why the icon should not be replaced by one. Worth noting that the button also doubles as the zoom-state indicator: it only appears when zoomed, so it tells you there is something to reset. A pure-gesture design loses that signal.

The one thing I would avoid is making the gesture the *only* way out. A user who zooms in by accident and cannot find the way back is stuck with a chart they cannot read.

`make check` is green: 33 backend tests, mypy clean, 51 frontend tests (9 new), typecheck, production build.

Pushed as `41b0aba` on `btakita/acadian-take-home`; superproject gitlink at `d50a0c4f4`.
### Re: double-click zoom reset — opus-4-8

> **Queue prompt:** Support double-click zoom reset.

Done — double-clicking anywhere on the plot resets it. The icon button stays, since it is the only *visible* affordance and it doubles as the zoom-state indicator. The button's tooltip now reads `Reset zoom (or double-click the chart)`, which is how the gesture gets discovered.

**The guard.** Drag-to-zoom and double-click-to-reset share the same pointer, so a zoom drag whose endpoints land close together would register as a double-click and immediately undo the zoom the user just made. The reset is therefore skipped when the preceding gesture travelled more than `DRAG_SLOP` (4px) — enough to absorb hand tremor in a real click, small enough that any deliberate selection clears it.

**Two defects the verification caught**, both of which I would have shipped on a code read:

*The guard never tripped.* I first derived travel distance from Recharts' `chartX`. Instrumenting the actual values showed `pressX: 100, latestX: 100` after a 100px drag — Recharts does not deliver `onMouseMove` while the button is held, so the travel distance stays pinned at its press value and **every** gesture classifies as a click. The guard was inert. It now reads native pointer coordinates from the container element, which is the right layer anyway: pointer travel is a DOM concern, not a chart concern.

*The brush labels were missing on first paint.* Chasing the first defect, I noticed the labels I shipped last cycle were absent entirely — no `.brush-labels` element at all. `ResponsiveContainer` settles its width asynchronously, so the brush can render *after* my layout effect runs; the effect bailed when the handles were not there yet and then waited on a resize that never came. It now retries on the next animation frame. This was a live flake in the previous commit, not a new regression — worth flagging, because the first time I saw it I assumed it was hot-reload noise and moved on. It was not.

**Verified end-to-end:**

| gesture | result |
|---|---|
| 100px drag, then immediate double-click | zoom preserved |
| stationary double-click (1px tremor) | resets to full range, button disappears |
| double-click at full range | no-op |
| labels after reload ×3 | present on all 7 cards |

One process note: the shared browser was being actively driven by another session mid-verification — tabs opening and reindexing under me, and at one point my own reload landed on someone else's page. Rather than keep grabbing tabs, I launched an isolated headless Chromium on a separate debug port for the rest of the run and shut it down afterwards. Worth knowing if concurrent sessions here often share that browser.

`make check` is green: 33 backend tests, mypy clean, 56 frontend tests (5 new), typecheck, production build.

Pushed as `94c2bc9` on `btakita/acadian-take-home`; superproject gitlink at `6fc7b3362`.

### Re: renamed to mag7-viewer, repo public — opus-4-8

> **Queue prompt:** Rename gh repo to mag7-viewer. Make sure documentation + code reflects new name. Rename submodule to mag7-viewer. make repo public

Done, all of it. **https://github.com/btakita/mag7-viewer** is public and anonymously cloneable.

- **Repo renamed** `acadian-take-home` → `mag7-viewer`. GitHub keeps the old URL redirecting, so nothing that referenced it breaks.
- **Submodule moved** to `src/mag7-viewer`, with the `.gitmodules` section name and URL both updated to match — `git mv` only moves the path and leaves the section name and URL stale, so those needed doing by hand.
- **Local plumbing repaired.** `git submodule status` was reporting the submodule as uninitialised (`-` prefix) because `.git/config` still keyed it under the old name and the gitdir pointer still aimed at `.git/modules/src/acadian-take-home`. Renamed the module directory, rewrote the `.git` pointer and `core.worktree`, and renamed the config section. Status is clean now.
- **Description updated** and the "take-home assessment" wording dropped, since the repo is public under a neutral name.
- **Visibility flipped to public**, verified anonymously rather than by trusting the API response: `curl` returns 200 unauthenticated, and a credential-less `git clone` of the public URL lands at `94c2bc9`, matching HEAD.

**No documentation or code inside the submodule changed** — worth stating explicitly rather than reporting a rename I did not need to do. The app's own identifiers were already `mag7-*` (`mag7-backend`, `mag7-frontend`, `<title>MAG7 Return Viewer</title>`, README titled "MAG7 Interactive Return Viewer"). The only place the old name lived was the repo URL and submodule path. I checked for clone URLs in the README and there are none.

**Before flipping visibility** I scanned the tracked file list and grepped for keys, tokens, private keys, and AWS ids. The only hit was the npm package `js-tokens` in `package-lock.json`. No credentials, no `.env`, no lockfile secrets.

**One mistake I made and corrected.** My first commit accidentally swept in three Vite dep-cache stubs (`frontend/.vite/deps*/package.json`) — a dev server was still running against the old path and recreated the directory underneath the rename, and my `git add src/acadian-take-home` picked it up. I stopped the three stale Vite processes, removed the files from the index, moved the leftover directory out of the tree, and amended. The commit is now exactly two entries: `.gitmodules` and the path rename.

Superproject commit `e30a275ba`. `make check` passes from the new path: 33 backend tests, mypy clean, 56 frontend tests, typecheck, production build.

Two loose ends I have not touched: the frontmatter still reads `agent: c` (harness-mismatch warning every preflight), and your next queue item — the reset button being misaligned — arrived while this was running and is unstruck.
<!-- agent:boundary:26f8ace9 -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- ~~Move the date selection tooltip below the selector ![img_7.png](img_7.png). Make sure the tooltip is always legible and does not obscure other UI data. Also handle when the two sliders are close together. Make sure the date tooltips do not overlap.~~
- ~~What is UX best practice to support resetting the zoom from the mouse? Don't implement anything, just discuss.~~
- ~~Support double-click zoom reset.~~
- ~~Rename gh repo to mag7-viewer. Make sure documentation + code reflects new name. Rename submodule to mag7-viewer. make repo public~~
- The reset button is misaligned ![img_8.png](img_8.png).
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done -->
- 2026-07-18 [#acad1] Scaffold `mag7-returns` repo: backend uv/FastAPI project + frontend Vite React-TS app, dev proxy, make dev/check, stub README (plan: tasks/recruit/acadian/plan.md)
- 2026-07-18 [#acad2] Backend: fetcher/returns/cache/api/models modules, typed, pytest with mocked yfinance, live endpoint verify (plan: tasks/recruit/acadian/plan.md)
- 2026-07-18 [#acad3] Frontend: useReturns hook, DateRangePicker, TickerCard grid (Recharts line + tooltip + Brush zoom + min/max/mean), loading + ErrorBanner (plan: tasks/recruit/acadian/plan.md)
- 2026-07-18 [#acad4] Bonus polish: cross-ticker SummaryTable, responsive pass, empty-range/API-down UX, stats unit tests (plan: tasks/recruit/acadian/plan.md)
- 2026-07-18 [#acad5] Submission: README (setup/run/assumptions), final make check, create + push public GitHub repo, clean-clone sanity test (plan: tasks/recruit/acadian/plan.md)
- 2026-07-20 [#backendsilentlydrops] Backend silently drops a requested ticker when yfinance returns no data for it (MSFT absent on the trailing-6-month range, present on a shorter range). Report unavailable symbols in the response payload instead of omitting them.
<!-- /agent:done -->
