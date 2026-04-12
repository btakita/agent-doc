---
agent_doc_session: a9421282-fd96-4943-9af5-3561ed5cb799
agent_doc_format: template
agent_doc_write: crdt
agent_doc_model_tier: low
claude_args: --dangerously-skip-permissions
---

## Status

<!-- agent:status patch=replace -->

<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Resolved shop page issues: (1) image heights via mu-plugin CSS + force-recreate (2) pagination removal with loop_shop_per_page=-1 (3) footer logo size matching (4) New Cloth Technologies aside hiding. All deployed to production, verified. Key insight: must-use plugins auto-load; regular plugins need admin activation.
### Re: "Popular" categories block restored

Rebuilt the block fresh (no trace in recent homepage revisions) and deployed repo-backed via mu-plugin seeder. Rendered live on https://50.28.2.199.nip.io/.

❯ The "Popular" categories block should have been added to the /shop/ page. Not the home page.
❯ The order needs to be:
❯ 1. Rod Holders
❯ 2. Rod Holder Bases
❯ 3. HydroWeb Landing Nets
❯ 4. River Anchors

**Block spec used:**
- Title: `Popular` (h2, centered, 32px — matches other section headings)
- 4-column `[product_categories]` shortcode, `orderby="menu_order"`, `hide_empty="0"`
- Category IDs in user order: `210,533,631,642`
  - 210 = Rod Holders
  - 533 = Rod Holder Bases
  - 631 = HydroWeb Landing Nets
  - 642 = River Anchors (note: slug is `anchors`, name is "River Anchors")
- Placement: immediately after the hero cover (`<!-- /wp:cover -->`), before Best Sellers

**What shipped (mrh-performance v1.8):**
1. Direct DB edit to `page_on_front` (post ID 2) to inject the block immediately
2. New mu-plugin `init` hook — idempotent seeder guarded by `mrh_home_popular_seed_version` option
   - Checks for the "Popular" heading marker; skips if present
   - Otherwise injects the block after `<!-- /wp:cover -->`
   - Falls back to prepend if no cover block exists
3. Pre-set `mrh_home_popular_seed_version = 2026-04-11-1` on the server so the seeder doesn't re-run after deploy
4. Submodule `560f5e2a` → rebased → `ba2d1c3b` pushed; parent pointer bumped
5. Server pulled, wordpress container force-recreated, cache flushed

**Live verification (`curl https://50.28.2.199.nip.io/`):**
- `>Popular<` heading: 1 hit ✅
- All 4 category names present in markup ✅
- 88 `product-category` class occurrences (shortcode rendered tiles) ✅

**Self-healing behavior:** If the homepage post gets reset (DB import, admin-UI edit wipe), bumping `MRH_HOME_POPULAR_SEED_VERSION` in the mu-plugin re-injects the block on next page load. Same pattern as the footer-logo seeder.

Hard-refresh the homepage (Cmd/Ctrl+Shift+R) to confirm visually.
### Re: Moved Popular block to /shop/ with strict order (HEAD)

Fixed. Reverted the homepage edit and put the block on the shop page (WooCommerce `woocommerce_shop_page_id` → post 6) with strict category ordering via 4-column layout.

**Why the columns layout:** WooCommerce's `[product_categories]` shortcode has an `orderby` parameter but it only accepts `name|count|id|slug|menu_order` — none of which honor the `ids` list order. To guarantee the user-specified sequence, I use a `wp:columns` block with one `[product_categories ids="N"]` shortcode per column. The columns layout preserves source order regardless of WC's internal sort.

**Column layout (strict order):**
1. Rod Holders (id 210)
2. Rod Holder Bases (id 533)
3. HydroWeb Landing Nets (id 631)
4. River Anchors (id 642)

**What I did:**

1. **Reverted homepage (post 2)** — restored original `post_content` from the pre-edit snapshot. `curl /` now shows 0 matches for `>Popular<`.
2. **Replaced shop page content (post 6)** — overwrote the broken existing shortcode (`ids="rod-holders,..."` — slugs aren't valid in `ids`) with the new Popular heading + 4-column tile row.
3. **Rewrote mu-plugin seeder (v1.9)** — now targets `woocommerce_shop_page_id` instead of `page_on_front`, builds the 4-column structure, guarded by `mrh_shop_popular_seed_version` option. Old `mrh_home_popular_seed_version` removed from server.
4. **Deployed** — submodule `f15ab36e` pushed, parent pointer bumped, container force-recreated, cache flushed.

**Live verification (positions after `>Popular<` heading in `curl https://50.28.2.199.nip.io/shop/`):**

| Order | Category | Offset after "Popular" |
|-------|----------|------------------------|
| 1 | Rod Holders | +1310 |
| 2 | Rod Holder Bases | +2539 |
| 3 | HydroWeb Landing Nets | +3834 |
| 4 | River Anchors | +5171 |

Sequence is correct — the 4 tiles render in the requested order between the "Popular" heading and the product loop. Hard-refresh `/shop/` to confirm visually.

**Note on the shop page layout:** WooCommerce renders the page's `post_content` via `woocommerce_archive_description` before the product loop, so the Popular block appears above the product grid. The product grid (51 products, no pagination from v1.4) continues below.
<!-- agent:boundary:7daa6bfd -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] **"Popular" categories block on /shop/** — 4-column strict-order layout (Rod Holders, Rod Holder Bases, HydroWeb Landing Nets, River Anchors), mrh-performance v1.9 seeder, deployed and verified
- [x] **Match footer logo size to header (~75px)** — CSS cap + srcset sizes fix in mrh-performance v1.6, deployed and verified
- [x] **Hide "New Cloth Technologies" aside column on /shop/** — mrh-performance v1.7, deployed and verified
<!-- /agent:pending -->
