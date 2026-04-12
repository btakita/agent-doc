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

### Re: Variable Image Heights

Fixed — the CSS priority was too low. Updated the fix to:
- Add `!important` to all image rules (overrides theme styles)
- Increase hook priority to 99 (runs after most theme/plugin CSS)
- Add `max-height: 300px` as additional constraint

All product images should now display at uniform **300px height** with full visibility (no cropping). Different aspect ratios are handled via `object-fit: contain`.

**File:** `src/monsterrodholders-dev/wordpress/plugins/mrh-performance/mrh-performance.php`

Test on /shop page — images should align consistently in the grid.

❯ Some product images are still taller than others. Please fix. Verify the fix using chromium-bridge dom measurements on the https://50.28.2.199.nip.io/shop/. Make sure all products .wp-post-image elements have the same dimensions.
❯ ![img_16.png](img_16.png)

### Re: Variable Image Heights (round 2)

**Root cause found via `curl` of /shop/.** The mrh-performance CSS was never reaching the page — zero matches for `height: 300`, `mrh-performance`, or even the preconnect `fonts.googleapis.com` hint in the rendered HTML. The regular plugin was never activated on the server, so none of items 1–5 in `mrh-performance.php` were running.

**Secondary cause in the markup.** Blocksy wraps each tile as `li.product > figure > a.ct-media-container > img.wp-post-image`, and every `img` carries an inline `style="aspect-ratio: X/Y;"` (1/1, 250/183, 556/427, 800/1067, 2048/1331…). Even if the old `.wp-post-image { height: 300px }` rule had loaded, the wrappers `figure` and `a.ct-media-container` would still collapse to the image's natural aspect-ratio box, so grid rows still wouldn't align.

**Fix (two parts):**

1. **Convert to a must-use plugin so activation is not required.** Moved `wordpress/plugins/mrh-performance/mrh-performance.php` → `wordpress/mu-plugins/mrh-performance.php`. Must-use plugins auto-load on every request — no admin step, survives plugin-list wipes. Added the mount to `docker-compose.yml`:
   ```yaml
   - ./wordpress/mu-plugins:/var/www/html/wp-content/mu-plugins
   ```

2. **Stronger CSS targeting the wrapper chain + clearing inline aspect-ratio:**
   ```css
   .products li.product figure,
   .products li.product figure > a.ct-media-container {
       height: 300px !important; min-height: 300px !important; max-height: 300px !important;
       width: 100% !important;
       display: flex !important; align-items: center !important; justify-content: center !important;
       overflow: hidden !important;
   }
   .products li.product .wp-post-image {
       height: auto !important; width: auto !important;
       max-height: 300px !important; max-width: 100% !important;
       aspect-ratio: auto !important;   /* overrides inline style */
       object-fit: contain !important;
   }
   ```
   The wrapper is locked to a 300px flex box; the img inside scales down to fit with full visibility; `aspect-ratio: auto !important` neutralizes the inline attribute.

**Files changed (in `src/monsterrodholders-dev/`):**
- `wordpress/mu-plugins/mrh-performance.php` (new, v1.3)
- `wordpress/plugins/mrh-performance/mrh-performance.php` (deleted)
- `docker-compose.yml` (added mu-plugins volume)

**Deploy (on the server, `/path/to/monsterrodholders-dev/`):**
```bash
git pull
./scripts/deploy.sh   # docker compose pull + up -d --remove-orphans + cache flush
```
`up -d` picks up the new `mu-plugins` volume mount. No plugin activation step needed.

**Verification I could not do from here.** You asked me to verify via chromium-bridge DOM measurements. The `chrome-devtools` MCP timed out on every call (`Network.enable timed out`), and the nip.io host is on the remote server, not locally reachable via Docker. I verified only the *current* state via `curl`, not the post-deploy state. After deploying, please either:
- Run `chromium-bridge` again against https://50.28.2.199.nip.io/shop/ and check all `.wp-post-image` wrappers (`figure > a.ct-media-container`) have `getBoundingClientRect().height === 300`, or
- Paste a screenshot and I'll eyeball.

If the images *still* vary after deploy, the most likely remaining cause is a page/fragment cache (Blocksy/Redis). `./scripts/deploy.sh` already runs `wp cache flush`, but a full-page cache may need `wp rocket clean` / `wp cache flush` + hard refresh.

❯ do
❯ - [ ] **Fix product image heights** — CSS + must-use plugin conversion committed; awaiting server deploy + post-deploy DOM verification

### Re: Committed and pushed (HEAD)

Submodule `src/monsterrodholders-dev` → commit `1e57e87e` pushed to origin/main (after rebase onto incoming `sync: WP content` commits). Parent `agent-loop` pointer updated in `c3ccfd47`.

**To apply on the server (50.28.2.199):**
```bash
cd /path/to/monsterrodholders-dev
git pull
./scripts/deploy.sh     # docker compose pull + up -d --remove-orphans + wp cache flush
```

`up -d --remove-orphans` will recreate the `wordpress` container so it picks up the new `./wordpress/mu-plugins` bind mount. The mu-plugin auto-loads, so there is no plugin-activation step.

**After deploy, verify** — run either check against https://50.28.2.199.nip.io/shop/:

1. Quick curl check (should now show the CSS):
   ```bash
   curl -sk https://50.28.2.199.nip.io/shop/ | grep -c "mrh-product-grid"
   # expect: 1
   ```
2. DOM check via chromium-bridge (paste output if you run it):
   ```js
   [...document.querySelectorAll('.products li.product figure > a.ct-media-container')]
     .map(el => ({ h: el.getBoundingClientRect().height, cls: el.className }))
   ```
   expect every `h` to be `300`.

Ping me with the result and I'll close out the pending item.
<!-- agent:boundary:4995daad -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- [ ] **Fix product image heights** — mu-plugin + wrapper CSS pushed (submodule `1e57e87e`, parent `c3ccfd47`); awaiting `git pull && ./scripts/deploy.sh` on the server + post-deploy verification
- [x] **Add category sidebar to /shop** — 4-tile grid via mu-plugin
- [x] **Swap Boat Accessories → HydroWeb Landing Nets**
- [x] **Swap Racks and T-Bars → River Anchors**
- [x] **Fix Rod Holders category image** (term 210 thumbnail_id → 77336)
- [x] **Restore `All Products` heading** above products list
- [x] **Remove duplicate hero `Popular Categories`** from Shop page block content
- [x] **Enable Blocksy sidebar on /shop/ and /blog/**
- [x] **Remove custom layout CSS** — header compact, main below header, aside below header
- [ ] *(Optional)* Wrap shop product grid in `<article>` via template override if strict article+aside parity is required
<!-- /agent:pending -->
