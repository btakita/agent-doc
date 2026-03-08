---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Phase 17 complete ✓ — SVG charts + PDF/XLSX export. Autocomplete ✓. Company A-Z ✓. Finnhub ✓. Portfolios ✓. WIL ✓. Next: benchmark data, mi-web deploy, esg-web deploy, custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

**Fixes deployed:**

1. **Distribution chart alignment** — Fixed the starting point of the cumulative curve. It was starting at (0,0) instead of the bottom-left of the plot area (`padding.left, padding.top + plot_h`). The curve now properly starts from the origin of the chart axes.

2. **XLSX formulas added** — The spreadsheet is now formula-driven:
   - **Weighted Average row** (SUMPRODUCT): `=SUMPRODUCT(Weight, Score)/SUM(Weight)` for each ESG issue
   - **Sum row**: `=SUM(Weight)` for total portfolio weight
   - **Rank row**: `=IF(avg>=0.8,"Excellent",IF(...))` — dynamic rank labels based on weighted averages
   - All formulas are live — editing scores/weights in the spreadsheet automatically recalculates

**Test:** Reload https://censible-root-web.brian-takita.workers.dev/portfolios/mi/report (chart fix) and re-download the XLSX (formulas).

I do like the xlsx export at https://censible-root-web.brian-takita.workers.dev/portfolios/mi/export.xlsx.
But I recall that the original xlsx export has many more columns. It was very sophisticated.
Can we replicate all of the features in another detailsed xlsx export. Let's keep this existing export as well.
<!-- /agent:exchange -->
