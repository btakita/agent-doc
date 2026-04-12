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
### Re: "Popular" categories block restored (HEAD)

Rebuilt the block fresh (no trace in recent homepage revisions) and deployed repo-backed via mu-plugin seeder. Rendered live on https://50.28.2.199.nip.io/.

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
<!-- agent:boundary:55d5ad25 -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] **Restore "Popular" categories block on homepage** — 4-category row (Rod Holders, Rod Holder Bases, HydroWeb Landing Nets, River Anchors), mrh-performance v1.8 seeder, deployed and verified
- [x] **Match footer logo size to header (~75px)** — CSS cap + srcset sizes fix in mrh-performance v1.6, deployed and verified
- [x] **Hide "New Cloth Technologies" aside column on /shop/** — mrh-performance v1.7, deployed and verified
<!-- /agent:pending -->
