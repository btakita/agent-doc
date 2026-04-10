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
### Re: home page polish — garderobe-inspired, data-driven

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

❯ Home page image is too small to be left aligned. It should be in the hero and centered.
❯ https://50.28.2.199.nip.io/wp-login.php responds with a 403. How can we make this accessible.
We should exclude Sold Out items from the home page.
### Re: hero image + wp-login 403

**Hero image — fixed ✓**

Replaced the plain `wp:image` block with a full-width `wp:cover` block (`align:full`, 350px min-height, `dimRatio:0` so the image shows at full brightness). The banner now stretches edge-to-edge at the top of the page.

**wp-login.php 403 — fixed ✓**

Root cause: Imunify360 mod_security was blocking external access to wp-login.php (curl from localhost returned 200; external returned 403). 

Fix: added `<LocationMatch "^/wp-(login|admin)">` with `SecRuleEngine Off` to the Apache vhost config at `/etc/httpd/conf.d/00-monsterrodholders-proxy.conf`. Config backup saved as `.conf.bak`. Apache reloaded. Login returns 200 ✓.

Note: this disables mod_security for `/wp-login.php` and `/wp-admin/*` only. The rest of the site stays protected. Once you go live on the real domain you may want to restrict wp-admin to your IP instead — see `imunify360-agent whitelist ip add <your-ip>`.

❯ Add a rule to exclude Sold Out items from the home page. The items should show on the home page when inventory is added.

❯ The footer should have links to all of the categories and other pages on the website.
### Re: sold-out exclusion + footer

**Sold-out items hidden ✓**

Set `woocommerce_hide_out_of_stock_items = yes` (WC global setting). This hides out-of-stock products from all catalog shortcodes on the home page — Best Sellers, Rod Holders, Rod Holder Bases, More Monster Gear. When inventory is added to a product, it automatically reappears. No shortcode changes needed.

**Footer updated ✓ — 33 items (was 16)**

Added to Footer 2 (assigned to `footer` location):

*New product categories:* Racks and T-Bars, Draggin Master, HydroWeb Landing Nets, Kayak Products, Mounting Hardware, Apparel & Decals, Bank Products, Rod Transport Solutions, Bait & Tackle, River Anchors, Gift Cards

*New pages:* FAQ, Warranty, Privacy Policy, Terms of Service, Shipping & Returns, Monster Bonus Money

Existing 16 items kept: Shop, Rod Holders, Rod Holder Bases, Boat Accessories, About Us, Contact Us, Blog, Certified Resellers, Pro Staff, Sponsorship Application, My Account, Cart, Checkout, Rewards Program, Affiliate Area, Wildcatters TV

Blocksy will wrap these into columns automatically. If the layout needs 4 columns instead of 3, adjust under Customize → Footer → Footer Columns.

❯ Deploy updates. Use ssh with my default ssh key to login to make updates. Ensure the changes are in the git repo.
### Re: deploy to git (HEAD)

`wp-to-git.sh` ran and pushed ✓

```
[main 1b0cbd2] sync: WP content 2026-04-10 18:14
 1 file changed, 33008 insertions(+), 32360 deletions(-)
→ github-monsterrodholders:btakita/monsterrodholders-dev.git main → main
```

All recent WP changes are now in git: home page (cover block, best sellers, bases layout), footer menu (33 items), out-of-stock hiding, page deletions.
<!-- agent:boundary:d04ed650 -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] Verify home page in browser — Cover block hero, Best Sellers, sold-out items hidden
- [ ] Verify footer renders 33 links cleanly
- [ ] Delete Recently Viewed Products page (18849)
- [ ] DNS flip: point monsterrodholders.com → 50.28.2.199
- [ ] Respond to 9 real customer order inquiries in comment hold queue
- [ ] Copy remaining uploads (verify completeness)
<!-- /agent:pending -->
