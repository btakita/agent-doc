---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Detailed XLSX export ✓ (5 sheets, formulas, conditional formatting). SVG charts ✓. PDF ✓. Autocomplete ✓. Finnhub ✓. Next: benchmark data, mi-web/esg-web deploy, custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

### Detailed XLSX Export ✓

Created a sophisticated 5-worksheet XLSX export replicating the original mi-web architecture:

**Route:** `/portfolios/:portfolio_id/export_detailed.xlsx`
**Test:** https://censible-root-web.brian-takita.workers.dev/portfolios/mi/export_detailed.xlsx

**5 Worksheets:**

| Sheet | Contents | Formulas |
|-------|----------|----------|
| **Fund** | Holdings with weights, scores, percentiles per issue | SUMPRODUCT weighted avgs, normalized weight (`=C/SUM(C)`), COUNTA criteria total |
| **ESG Detail** | Full issue scores + percentiles for all companies | AVERAGE per column |
| **Criteria** | Editable issue weights (yellow cells) | SUM of weights |
| **Peers** | Cross-reference between all holdings pairs | MAXIFS, COUNTIF, peer percentile (`=score/max`) |
| **Summary** | Dashboard with portfolio metrics | References Fund sheet, IF-based rank labels |

**Styling:**
- 25 cell styles: rank 1-5 conditional fill colors, header formatting, editable yellow bg
- Frozen panes (row 1 + column A) on all sheets
- Number formats: 0.0 weights, 0.00 scores, 0% percentiles
- Bold totals with double borders

Both exports available:
- Simple: `/portfolios/:id/export.xlsx`
- Detailed: `/portfolios/:id/export_detailed.xlsx`

The Peers sheet columns D and E have `#NAME?` errors.
In the Fund sheet, the ESG score columns should use a formula pointing to the ESG Details sheet
<!-- /agent:exchange -->
