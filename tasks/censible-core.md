---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** USB drive mounted ✓. Found 631MB of CSV data (ESG scores, company data, WIL, MSCI). Copied to `src/censible-core/data/import/`. Ready for D1 import.
<!-- /agent:status -->

<!-- agent:exchange -->

### USB Drive Results

Mounted the Best Buy NVMe enclosure (`/dev/sda`):
- **sda2** (ext4, 99.6G) — old Linux root filesystem
- **sda3** (btrfs, 376.8G) — home directory with `work/portfoliome/censible-core/`

**Found 631MB of CSV data** at `censible-core/data/import.mi-www.site/`:

| File | Size | Rows | Content |
|------|------|------|---------|
| `esg_detail.csv` | 11M | 9,057 | Full company ESG scores (30 key issues × score + percentile) |
| `company_gender.csv` | 2.2M | 9,058 | Gender diversity metrics per company |
| `iex__company.csv` | 5.8M | 11,396 | Company master data (ticker, name, industry, exchange) |
| `_msci__esg.csv` | 5.8M | 9,057 | MSCI ESG raw scores |
| `company_person.csv` | 263M | — | Executive/person data (large) |
| `esg_mi_wil_*.csv` | 30K×4 | 17 each | WIL portfolio positions by date |
| `esg_mi_eco_*.csv` | 40K×4 | — | Eco portfolio positions by date |
| `geographic_market_classification.csv` | 8.7K | 248 | Country classifications |
| `esg_*_percentile1.csv` | 3-4M×5 | — | Percentile rankings by dimension |

Copied all to `src/censible-core/data/import/import.mi-www.site/`.

### Data Architecture Understanding

These CSVs are the **mi-web import pipeline** data, NOT raw finegar PostgreSQL dumps:

1. **MSCI raw data** → `msci_detail_stage0.csv` → `msci_detail_stage1.csv`
2. **Percentile computation** → `esg_*_percentile1.csv` (geographic, industry, materiality, peer, SIC)
3. **ESG detail build** → `esg_detail.csv` (weighted scores across 5 dimensions, ~9K companies)
4. **MI portfolio enrichment** → `esg_mi_*.csv` (positions + ESG scores)

The mi-web app loaded these CSVs at SSR time via PapaParse streaming — no database needed.

For ESG data, the more recent csv files may have all of the info that we need.
Some changes may be necessary, but it should give us what we need.

### Plan: D1 Import from CSVs

The key files for D1 import (search + company data):
- `esg_detail.csv` — 9K companies with ESG scores (the core search/company data)
- `iex__company.csv` — 11K companies with ticker, name, industry, exchange
- `company_gender.csv` — gender diversity metrics
- `geographic_market_classification.csv` — country reference data

**Next steps:**
1. Design D1 schema for company search + ESG data
2. Write CSV→D1 import script
3. Implement search endpoints in the Worker (replacing finegar API proxy)
4. Wire up company detail pages with live ESG data

Shall I proceed with the D1 schema design and import?

Yes
<!-- /agent:exchange -->
