---
agent_doc_session: 7605b2c4-eaba-4a4c-9739-bb52a499521e
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
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
<!-- agent:boundary:424725bb -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#acad1] Scaffold `mag7-returns` repo: backend uv/FastAPI project + frontend Vite React-TS app, dev proxy, make dev/check, stub README (plan: tasks/recruit/acadian/plan.md)
- [ ] [#acad2] Backend: fetcher/returns/cache/api/models modules, typed, pytest with mocked yfinance, live endpoint verify (plan: tasks/recruit/acadian/plan.md)
- [ ] [#acad3] Frontend: useReturns hook, DateRangePicker, TickerCard grid (Recharts line + tooltip + Brush zoom + min/max/mean), loading + ErrorBanner (plan: tasks/recruit/acadian/plan.md)
- [ ] [#acad4] Bonus polish: cross-ticker SummaryTable, responsive pass, empty-range/API-down UX, stats unit tests (plan: tasks/recruit/acadian/plan.md)
- [ ] [#acad5] Submission: README (setup/run/assumptions), final make check, create + push public GitHub repo, clean-clone sanity test (plan: tasks/recruit/acadian/plan.md)
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
