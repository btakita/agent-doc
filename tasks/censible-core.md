---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Autocomplete search live ✓. Phase 15 complete ✓. Finnhub ✓. Next: mi-web portfolio reports (SVG charts vs tables TBD), esg-web, custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

### Phase 16 — Autocomplete Search ✓ + mi-www.site Analysis

**Autocomplete Search** — deployed:
- Live search dropdown as you type (debounced 300ms, fetches `/api/search`)
- Keyboard navigation (arrows, Enter, Escape)
- Click-to-navigate to company page
- Falls back to normal form submit if no selection
- **Test:** https://censible-root-web.brian-takita.workers.dev/companies

**mi-www.site Feature Inventory** (from source code analysis):

The original mi-web is a SolidJS SSR app generating ESG portfolio reports. Key features:

| Feature | Description |
|---------|-------------|
| **Portfolio ESG Reports** | 6 portfolio pages (3 Eco + 3 WIL snapshots) with SVG bar charts and line graphs |
| **Score Visualizations** | Portfolio summary bar, per-issue bars comparing vs benchmarks (URTH, ERTH, PXWEX) |
| **Two Criteria** | Eco (carbon, water stress) and WIL (gender diversity) with weighted scoring |
| **PDF Export** | Multi-page PDF via pdfkit + svg-to-pdfkit (5 pages: ordinal/raw scores + graphs) |
| **XLSX Export** | Multi-sheet workbook with positions, ESG detail, criteria weights, peer comparisons |
| **SVG Export** | Multi-page SVG document |
| **Raw Data Table** | All positions with ticker, weight, per-issue scores, rank 1-5 color coding |
| **Print Mode** | `?print` query param for print-optimized CSS |
| **Benchmark Comparison** | Each portfolio compared against URTH, ERTH, PXWEX benchmark ETFs |

**Data model:** CSV-sourced positions with per-issue scores/percentiles, peer comparisons (geographic, industry, materiality, SIC), market-weight-normalized aggregation.

**Replication approach for Hono mi-web:**
1. We already have the portfolio data in JSON (Phase 13 portfolios with ESG scores)
2. SVG chart generation can be server-side (template strings like current HTML pages)
3. PDF export: printer-friendly HTML approach (like our report.ts) since pdfkit won't work in Workers
Would svg to pdf work? Let's do that if it will work.
4. XLSX: can use a lightweight XML approach (no archiver needed)
Yes
5. Benchmark data needs to be imported from CSV or computed from D1
Yes

What's your priority — replicate the full SVG charts, or start with the data tables + print-to-PDF approach we already have?

Full SVG charts. SVG -> PDF.
<!-- /agent:exchange -->
