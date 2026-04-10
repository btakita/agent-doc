---
agent_doc_session: a9421282-fd96-4943-9af5-3561ed5cb799
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
WordPress migration to Docker on 50.28.2.199 complete. Stack: wordpress:apache, Apache reverse-proxy, SSL certbot, table prefix wp_nxz1mfc9cg_. Theme: Blocksy + Blocksy Companion Pro 2.1.38. WPBakery replaced with Gutenberg blocks (SEO preserved). git↔WP sync via wp-to-git.sh + read-write deploy key monsterrodholders_deploy_rw → src/monsterrodholders-dev. Nav: 4 core links (Shop, Blog, About, Contact) + Footer 2 menu. 38 demo/GeoDirectory pages deleted; hero disabled; single column layout set. This session: deleted On Sale (5531), Bestsellers (7519), Hot Deal (6987), New Metallic Look draft (15892). Open: Recently Viewed Products (18849) — WC shortcode page, deletion pending. Pending: 9 customer order inquiries in comment hold queue; copy remaining uploads.

❯ Use https://startersites.io/blocksy/garderobe/ as an inspiration for product layout. monsterrodholders has an extensive product category, so consider tradeoffs with the design vs garderrobe, which is for clothing.
❯ ron-berkes likes how friendly it is for ecommerce shopping. Let's have some polish for monsterrodholders. Make it so people want to buy.
❯ Look at the top sellers in the database. Emphasize what sells. Rod Holders are very popular and tier 1.
### Re: home page polish — garderobe-inspired, data-driven (HEAD)

**Top sellers (from the actual database):**
1. Double Action 33/45 O.G. — 36K units (the flagship)
2. Double Action Talon 5/35 — 13K
3. Slide Track 45 Base — 10K
4. Economy Base 2 + Economy Base 4 — 9.5K each
5. Slide Track 90 / Versa Track — 7.5K

Rod Holders are clearly the pull product. Bases have 17 SKUs across boat types (pontoon, jon boat, SeaArk, round rail, square rail) — compatibility is the purchase barrier for that category.

**What garderobe does well (transferable):**
- Clean section separation with centered headings
- Leads with what converts (not generic welcome text)
- Products above the fold before any other content
- Compatibility/category navigation reduces decision friction

**Changes made to home page (post 7503):**
- **Removed** the 25/75 column hack — header image is now full-width
- **Removed** broken `/bestsellers/` image link (that page was deleted last session — it was a 404)
- **Added** "Best Sellers" section at top using actual top-5 by sales: IDs 8642, 22608, 8581, 8635, 8633 — 5-column grid
- **Rod Holders** section kept (ordered by rating)
- **Rod Holder Bases** section — added compatibility subtitle: "Fits pontoons, jon boats, SeaArk, Versa Track..." + expanded to 12 products (was 8)
- **"More Monster Gear"** replaces the all-products dump — ordered by popularity

**What would further improve it (next pass):**
- A full-width cover/banner block above fold (image + headline + "Shop Now" CTA) — garderobe's above-fold CTA pattern
- "Shop by Boat Type" filter shortcut for the bases category (this is where compatibility friction lives)
- Verify the 5-column Best Sellers layout looks right on mobile (may want 3 columns on small screens — requires CSS or a Blocksy column override)
<!-- agent:boundary:4224bd3b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Verify home page layout looks right in browser (Best Sellers 5-col, bases compatibility text)
- [ ] Consider above-fold cover block with headline + "Shop Now" CTA
- [ ] Delete Recently Viewed Products page (18849) — confirmed as WC session shortcode, not useful standalone
- [ ] Respond to 9 real customer order inquiries in comment hold queue
- [ ] Copy remaining uploads (verify completeness)
<!-- /agent:pending -->
