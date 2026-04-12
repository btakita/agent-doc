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
WordPress migration, Docker stack, layout refinements (hero, grids, sticky sidebar), DNS flip pending certbot
❯ The product .wp-post-image elements need to all have the same height. Resize the image so entire image is visible within the .wp-post-image.

❯ Some products have taller .wp-post-image elements than the others.
❯ ![img_15.png](img_15.png)
❯ ![img_14.png](img_14.png)

### Re: Variable Image Heights (HEAD)

Fixed — the CSS priority was too low. Updated the fix to:
- Add `!important` to all image rules (overrides theme styles)
- Increase hook priority to 99 (runs after most theme/plugin CSS)
- Add `max-height: 300px` as additional constraint

All product images should now display at uniform **300px height** with full visibility (no cropping). Different aspect ratios are handled via `object-fit: contain`.

**File:** `src/monsterrodholders-dev/wordpress/plugins/mrh-performance/mrh-performance.php`

Test on /shop page — images should align consistently in the grid.
<!-- agent:boundary:55d335f1 -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- [x] **Add category sidebar to /shop** — 4-tile grid via mu-plugin
- [x] **Swap Boat Accessories → HydroWeb Landing Nets**
- [x] **Swap Racks and T-Bars → River Anchors**
- [x] **Fix Rod Holders category image** (term 210 thumbnail_id → 77336)
- [x] **Restore `All Products` heading** above products list
- [x] **Remove duplicate hero `Popular Categories`** from Shop page block content
- [x] **Enable Blocksy sidebar on /shop/ and /blog/** (left, matching home page layout)
- [x] **Remove custom layout CSS** — header compact, main below header, aside below header
- [x] **Fix product image heights** — consistent 300px height with !important CSS override
- [ ] *(Optional)* Wrap shop product grid in `<article>` via template override if strict article+aside parity is required
<!-- /agent:pending -->
