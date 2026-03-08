---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Detailed XLSX export ✓ (5 sheets, cross-sheet formulas, conditional formatting). SVG charts ✓. PDF ✓. Autocomplete ✓. Finnhub ✓. Next: benchmark data, mi-web/esg-web deploy, custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

### XLSX Detailed Export Fixes ✓

**Peers sheet #NAME? fix:** Replaced `MAXIFS` with `MAX(range)` using contiguous row groups per ticker. Simplified IF formula to avoid OOXML quoting issues.

**Fund sheet cross-sheet formulas:** Issue score/percentile cells now reference ESG Detail via `='ESG Detail'!{col}{row}` formulas instead of static values. Editing ESG Detail automatically updates Fund.

**Formula escaping fix:** `cell_formula` now only escapes `&` and `<` — `>` and `"` are valid in OOXML formula text.

**Test:** https://censible-root-web.brian-takita.workers.dev/portfolios/mi/export_detailed.xlsx

Looks good. Anything else for the migration from Contabo to Cloudflare?
<!-- /agent:exchange -->
