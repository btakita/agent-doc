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
Exchange compaction: consolidates shop page layout iterations (v1.8-v1.17), Popular categories section fixes, sidebar styling, and PayPal/CRS investigation. Preserved: design decisions (hook-based rendering vs post_content), markup fixes (ul removal), CSS selectors, and open testing item.
❯ Implement /wp-sitemap.xml? /sitemap.xml should redirect to /wp-sitemap.xml

❯ Compare http://50.28.2.199.nip.io/wp-sitemap.xml with https://monsterrodholders.com/wp-sitemap.xml

❯ Make sure when we transition we will not have any negative SEO affects.

❯ done
❯ - **Retry /checkout → PayPal** — confirm PayPal order creation reaches the approval flow after the `form_encoded` exclusion. (user action)
❯ - **Retry /checkout with test product + Stripe** — confirm Free Shipping / card path still works (regression check after CRS reload). (user action)

❯ do
❯ - **Livestream VOD trim** — the Stripe settings dump segment still needs manual trim in post-production.


### Re: Sitemap comparison + SEO safety for transition

**Test results** (staging is HTTPS only — http://50.28.2.199.nip.io redirects to https://):

| URL | Staging (nip.io) | Prod (monsterrodholders.com) |
|---|---|---|
| `/wp-sitemap.xml` | 301 → `/sitemap_index.xml` | **200** (WP core sitemap) |
| `/sitemap.xml` | 301 → `/sitemap_index.xml` | 301 → `/wp-sitemap.xml` |
| `/sitemap_index.xml` | **200** (Yoast SEO) | — |
| `robots.txt` Sitemap: | `/sitemap_index.xml` | *(none declared)* |

**Root cause of the divergence:** staging has **Yoast SEO** active and handling sitemaps at `/sitemap_index.xml`. Prod is using **WordPress core's built-in sitemap** at `/wp-sitemap.xml` with no Yoast (or Yoast's sitemap feature disabled). Yoast hooks `init` to take over `/wp-sitemap*` URLs and 301-redirect them to its own `/sitemap_index.xml`, which is why staging's `/wp-sitemap.xml` returns 301 instead of a sitemap index.

**The two sitemaps differ in more than URL:**
- Prod (core) lists 8 sub-sitemaps: posts, pages, products, category, post_tag, product_cat, product_tag, product_shipping_class.
- Staging (Yoast) lists 9 — all of the above **plus `author-sitemap.xml`**. Yoast includes author archives by default; core doesn't.

**SEO risk assessment for the DNS flip:**

1. **Indexed URLs**: whatever Google currently has indexed as the canonical sitemap is `/wp-sitemap.xml` (the only one that returns 200 on prod today). Since prod's robots.txt doesn't declare a Sitemap: directive, Google is either using Search Console submission or autodiscovery at `/wp-sitemap.xml` / `/sitemap.xml`. After the flip, staging's Yoast will 301 both of those to `/sitemap_index.xml` — Google follows 301s for sitemaps, so it will update its reference. **Short-term: no ranking loss, just a crawl refresh.**

2. **Author archives leak**: Yoast's `author-sitemap.xml` exposes author user slugs (typically `brookebrodack` or similar). This is a minor privacy/security footprint and an SEO no-op for a storefront. **Recommend disabling author archives in Yoast** (SEO → Search Appearance → Archives → Author archives: Disabled) before the flip, or excluding author-sitemap via Yoast filter. That also 404s `/author/<slug>/` pages, which is what you want for a commerce site.

❯ The author has a blog the website. Given we have a blog of a popular YouTube channel, should we keep the author slug?
❯ https://www.youtube.com/@SteveDouglastheCatfishDude (119k subscribers)

3. **Search Console re-submission**: after the DNS flip, submit `https://monsterrodholders.com/sitemap_index.xml` in Search Console (or add `Sitemap: https://monsterrodholders.com/sitemap_index.xml` to robots.txt so Google autodiscovers it). This is the cleanest signal that the sitemap location has moved.

4. **Canonical domain**: current staging XSL stylesheet is protocol-relative (`//50.28.2.199.nip.io/...`) because Yoast generates URLs from the request `Host` header. Since `ProxyPreserveHost On` is set in the Apache vhost, after the DNS flip Yoast will automatically emit `monsterrodholders.com` URLs without any config change. **No action needed here**; the proxy already does the right thing.

**Do you need to "implement /wp-sitemap.xml"?** No — it already exists and already 301s to the Yoast sitemap. The question is whether you want to keep Yoast (staging's current state) or revert to WP core (prod's current state):

- **Keep Yoast** (recommended): richer sitemap, meta tags, breadcrumbs, Open Graph. Disable author archives. Re-submit `/sitemap_index.xml` to Search Console post-flip.
- **Revert to core**: deactivate Yoast on staging, flush rewrites (`wp rewrite flush`). Pro: sitemap URL matches what Google has indexed today. Con: you lose all of Yoast's other SEO features.

**My recommendation**: keep Yoast, disable author archives, and add `Sitemap: https://monsterrodholders.com/sitemap_index.xml` to robots.txt once the flip is done. Net SEO impact during transition: neutral to positive (Yoast generally improves schema markup and meta coverage vs core-only).

❯ write the SEO report to a private gist

### Re: Livestream VOD trim

Noted — that's a manual post-production step on your end. I've left it in pending.

### Re: SEO report → private gist

Created as a secret gist:

**https://gist.github.com/btakita/f02a1aba87607629ca158c4098b4a39a**

Contents: full sitemap comparison table, root cause (Yoast vs WP core), structural diff, 4-point SEO risk assessment (indexed URLs, author archives leak, Search Console resubmission, canonical domain handling), recommendation, and action items. Local copy at `/tmp/monsterrodholders-sitemap-seo-report.md`.

Secret gists are unlisted but anyone with the URL can view — don't share the link publicly if any of the content is sensitive. Nothing in the report exposes credentials or internal infra beyond what's already in the task doc.

❯ /wp-sitemap.xml, /sitemap.xml, and /sitemap_index.xml all have 404s. Fix this.
### Re: Sitemap 404s — can't reproduce, need host

Just probed both hosts from here. Current reality:

| URL | prod (monsterrodholders.com → Flywheel) | staging (50.28.2.199.nip.io → our Docker) |
|---|---|---|
| `/wp-sitemap.xml` | **200** | 301 → `/sitemap_index.xml` |
| `/sitemap.xml` | 301 → `/wp-sitemap.xml` | 301 → `/sitemap_index.xml` |
| `/sitemap_index.xml` | **404** | **200** (Yoast) |

So only one URL on one host is currently broken (`/sitemap_index.xml` on prod), and that's expected — prod is still running WP core sitemaps via Flywheel, no Yoast. All three resolve cleanly on staging.

**Which host are you seeing all three 404?** A few possibilities:

1. **DNS cache / different network** — you may be resolving `monsterrodholders.com` to our Docker server (50.28.2.199) while Apache still has `ProxyPass` sending `/sitemap*` traffic to Flywheel upstream for that Host header. I just confirmed that from the server itself: `curl -H 'Host: monsterrodholders.com' http://localhost/sitemap_index.xml` 301s to prod, which 404s. If your machine resolves monsterrodholders.com → 50.28.2.199, you'd see: wp-sitemap→prod 200, sitemap→prod 301→wp-sitemap 200, sitemap_index→prod 404. Only one 404, not three.

2. **A third host I don't know about** — e.g. a laptop/local docker, a different preview URL, CF tunnel, etc. If so, tell me the host and I'll probe it.

3. **Browser cache / service worker** — unlikely for XML but possible if you previously hit these URLs and a SW cached a 404.

Can you paste the exact URLs (with scheme + host) you're hitting, or run this from the same machine you're testing on and share output:

```bash
for u in wp-sitemap.xml sitemap.xml sitemap_index.xml; do
  echo "=== $u ==="
  curl -sI "https://monsterrodholders.com/$u" | head -3
done
```

Once I know which host, the fix is straightforward:

- **Staging (nip.io)**: already fine, nothing to do.
- **Prod Flywheel `/sitemap_index.xml` 404**: this is the known Yoast-vs-core split we already discussed. Fix is either (a) install Yoast on prod (unlikely you want to touch the Flywheel site pre-flip), or (b) ignore it until DNS flip — post-flip the staging Yoast will handle all three URLs.
- **DNS-resolved-to-staging scenario**: I can add Apache rewrites on our Docker server so that when Host=`monsterrodholders.com`, `/sitemap_index.xml` is served locally by the container (Yoast) instead of being proxied to Flywheel. Would eliminate the 404 you'd see during a partial/split DNS state.

Let me know which host and I'll apply the actual fix.
❯ 404s
❯ - http://50.28.2.199.nip.io/sitemap.xml
❯ - http://50.28.2.199.nip.io/wp-sitemap.xml
❯ - http://50.28.2.199.nip.io/sitemap_index.xml
### Re: HTTP (port 80) sitemap 404s — fixed

You were right, and I was looking at the wrong scheme. All three URLs were 404 over **http://** (port 80) on the nip.io host. HTTPS worked fine, which is why my first probe missed it.

**Root cause:** Apache vhost binding precedence.

Our `/etc/httpd/conf.d/00-monsterrodholders-proxy.conf` had the port-80 redirect vhosts declared as `<VirtualHost *:80>`, but the InterWorx-managed `vhost_000_defaults.conf` has an IP-specific vhost on `<VirtualHost 50.28.2.199:80>` (serving `interworx-default-sites.com` from `/var/www/html`). Apache prefers IP-specific vhosts over wildcard `*:80` when matching a request, so every HTTP request to 50.28.2.199:80 was hitting the InterWorx default — which has no docroot for sitemap files, hence 403 on `/` and 404 for everything else. Our `Redirect permanent / https://…` rules were never reached.

`httpd -S` confirmed the split:
```
50.28.2.199:80   interworx-default-sites.com (vhost_000_defaults.conf:14)
*:80             default server 50.28.2.199.nip.io (00-monsterrodholders-proxy.conf:2)
                 port 80 namevhost monsterrodholders.com
```

**Fix applied:** Changed both port-80 vhosts in `00-monsterrodholders-proxy.conf` from `<VirtualHost *:80>` to `<VirtualHost 50.28.2.199:80>`. That puts them on the same IP-bind as the InterWorx default, and since `00-*` loads before `vhost_000_*` alphabetically, our `50.28.2.199.nip.io` vhost becomes the default for 50.28.2.199:80 and name-based matching resolves `monsterrodholders.com` and `www.monsterrodholders.com` correctly. `interworx-default-sites.com` still works by ServerName.

Sequence:
```bash
cp 00-monsterrodholders-proxy.conf 00-monsterrodholders-proxy.conf.bak-20260412
sed -i 's|<VirtualHost \*:80>|<VirtualHost 50.28.2.199:80>|g' 00-monsterrodholders-proxy.conf
httpd -t  # Syntax OK
systemctl reload httpd
```

**Verification:**
```
http://50.28.2.199.nip.io/                  → 301 → https://50.28.2.199.nip.io/
http://50.28.2.199.nip.io/sitemap.xml       → 301 → https://50.28.2.199.nip.io/sitemap.xml
http://50.28.2.199.nip.io/wp-sitemap.xml    → 301 → https://50.28.2.199.nip.io/wp-sitemap.xml
http://50.28.2.199.nip.io/sitemap_index.xml → 301 → https://50.28.2.199.nip.io/sitemap_index.xml
```
Following any of them lands at `https://50.28.2.199.nip.io/sitemap_index.xml` → 200 (Yoast).

**Side effect to be aware of:** `http://monsterrodholders.com/` and `http://www.monsterrodholders.com/` will also now redirect to https via our vhost **if and only if** DNS points the host at 50.28.2.199. Today DNS still points to Flywheel, so prod HTTP traffic is unaffected — Flywheel handles its own HTTP→HTTPS. After the DNS flip, our vhost will correctly 301 HTTP to HTTPS for both nip.io and the apex.

Backup is at `/etc/httpd/conf.d/00-monsterrodholders-proxy.conf.bak-20260412` in case you want to revert.

One more thing worth noting: the user-side symptom for the three URLs was 404 (not 403), which was because `mod_rewrite` on the InterWorx default would try to serve the literal path `/sitemap.xml` out of `/var/www/html` and fail. Root hit 403 because `/var/www/html` has directory listing disabled and no index file. Both symptoms traced back to the same vhost-selection bug.

### Re: author slug for a 119k-sub YouTuber's blog

Yes — keep Steve's author archive. For a creator-driven blog, `/author/steve-douglas/` is an SEO **asset**, not a leak: it's a topical landing page Google can rank for "Steve Douglas catfish" queries, and it concentrates E-E-A-T signals (author → posts → channel) that product/category pages can't.

**What to do:**

1. **Audit WP users first.** Run `wp user list --role=author,editor,administrator --fields=ID,user_login,display_name,user_email`. The concern I flagged earlier (`brookebrodack` slug leak) only matters if dev/staff accounts are also authoring posts. If Steve is the only public byline, you're fine.

2. **Clean the slug.** Set his `user_nicename` to `steve-douglas` (or `catfishdude` to match the channel handle) so the URL is `/author/steve-douglas/` not `/author/stevedouglas123/`:
   ```bash
   wp user update <id> --user_nicename=steve-douglas
   ```

3. **Exclude non-Steve authors from the sitemap** instead of disabling author archives globally. Two options:
   - Yoast → SEO → Search Appearance → Archives → Author archives: **Enabled**, then "Show archives for users without posts: Disabled" (hides brookebrodack/admin if they have zero published posts).
   - Or a `wpseo_sitemap_exclude_author` filter that returns all user IDs except Steve's.

4. **Wire up E-E-A-T signals on his author page.** Edit his WP profile:
   - Bio: short intro mentioning the YouTube channel by name
   - Website: `https://www.youtube.com/@SteveDouglastheCatfishDude`
   - Add `rel="me"` link in the theme's author template pointing to the channel (helps Google connect entity → channel → storefront)
   - Optional: Yoast → Schema → Person, mark him as the site's Person entity so `Article` schema on blog posts cites him as author

5. **Noindex anything that isn't Steve.** If you end up with one or two staff accounts with posts you don't want indexed, use `wpseo_sitemap_exclude_author` + add them to `noindex` via a `wpseo_robots` filter for their author URLs.

**Bottom line:** the earlier "disable author archives" advice assumed a generic commerce storefront. With a 119k-sub creator attached, author archives flip from footprint to moat — keep them, just make sure only Steve shows up.

❯ Are there any differences of site structures between monsterrodholders.com and the staging site?
### Re: site structure diff — prod vs staging

**I don't know the full diff without crawling both** — I can only tell you what's surfaced in this session so far. Here's what I know and a concrete way to get the rest.

**Known differences (from this session's work):**

1. **Sitemap plugin:** staging has Yoast active and emits `/sitemap_index.xml`; prod is on WP-core `/wp-sitemap.xml` with Yoast either inactive or not configured to take over. This is the only confirmed content-surface difference.
2. **Author archives:** tied to #1 — staging exposes `/author/<slug>/` via Yoast's author-sitemap; prod behavior depends on whether author archives are enabled in the WP core generator.
3. **Host/proxy plumbing:** staging is fronted by `50.28.2.199.nip.io` via Apache `ProxyPreserveHost` to the Docker container. Prod presumably terminates `monsterrodholders.com` directly. Not a *site structure* difference, but it affects how canonical URLs render.

**What I don't know (and can't infer without data):**

- Whether prod and staging have the same **pages** (cart, my-account, checkout, homepage) with the same IDs and slugs. Earlier in this doc you migrated cart and my-account on staging — unclear if prod was ever migrated the same way.
- Whether the **product catalog, categories, and menus** are in sync, or if staging has drifted (test products, renamed categories, etc.).
- Whether **active plugins** match (Yoast is one known delta; there may be others).
- Whether **theme / template overrides** differ.
- Whether **permalink structure** matches.

**How to get a real diff in ~5 minutes:**

Run these three commands on both sides and `diff` the outputs. You can hit staging via the Docker container and prod via `ssh monsterrodholders-server` (prod) — or use `wp` over SSH if you have credentials for both.

```bash
# 1. Active plugins
wp plugin list --status=active --fields=name,version > plugins.txt

# 2. Page inventory (post_type=page, published)
wp post list --post_type=page --post_status=publish --fields=ID,post_title,post_name > pages.txt

# 3. Key WooCommerce / homepage options
wp option get woocommerce_cart_page_id
wp option get woocommerce_checkout_page_id
wp option get woocommerce_myaccount_page_id
wp option get page_on_front
wp option get show_on_front
wp option get permalink_structure
wp option get template
wp option get stylesheet
```

Dump both to files and `diff -u prod-plugins.txt staging-plugins.txt` etc. That gives you a definitive structural delta.

**Blocker:** if you're still locked out of prod admin (password reset pending from earlier in this doc), you'd need to run these as the root `wp` user via SSH on the prod host. Want me to write a one-shot script that SSHes into prod, pulls all three inventories, does the same against the staging container, and diffs them?

❯ Can you compare the staging sitemap with monsterrodholders.com/wp-sitemap.xml? Make sure the urls all match.
### Re: sitemap URL diff — prod vs staging (HEAD)

Fetched both sitemap indices and all child sitemaps, extracted every `<loc>`, normalized hosts, and diffed the URL sets. **The sitemaps do NOT match** — there are meaningful drifts in three categories, some of which are actually good (staging is cleaner), and two of which are red flags on prod that need cleanup before the DNS flip.

**Summary:**

| Category | Prod | Staging | Common | Drift |
|---|---|---|---|---|
| posts | 25 | 10 | 9 | 🔴 16 prod-only, 1 staging-only |
| pages | 79 | 40 | 31 | 🟡 48 prod-only, 9 staging-only |
| products | 57 | 59 | 57 | 🟢 2 staging-only (test + /shop) |
| category | 3 | 2 | 2 | 🔴 1 prod-only (`/category/casino`) |
| post_tag | 11 | 11 | 11 | ✅ match |
| product_cat | 17 | 17 | 17 | ✅ match |
| product_tag | 220 | 220 | 220 | ✅ match |
| product_shipping_class | 2 | 2 | 2 | ✅ match |
| author | — | 1 | — | Yoast-only, needs slug cleanup |

**Taxonomies are clean:** product_cat (17), product_tag (220), post_tag (11), product_shipping_class (2), and most of the category taxonomy are identical. All 57 prod products exist on staging. That's the good news — the commerce catalog is in sync.

**🔴 RED FLAG #1 — prod has 16 SEO-spam blog posts that staging lacks:**

```
/2026/03/26/beliebte-sportarten-der-deutschen-bei-sportwetten-ohne-oasis-...
/2026/03/26/wettgeschichten-und-die-ubersicht-uber-sportwetten-ohne-lugas
/2026/03/27/najlepsze-aplikacje-do-zakladow-sportowych-za-darmo
/2026/03/27/poshagovoe-rukovodstvo-kak-rabotaet-vyvod-sredstv-cherez-1xbet-vkhod
/2026/03/28/plinko-casinos-2026-on-the-web-casinos-mit-unserem-plink-game-...
/2026/03/28/spielen-sie-plinko-um-geld-2026
/2026/03/29/obzor-pin-up-mobilnogo-prilozheniia-igraite-v-onlain-igru-...
/2026/03/29/pinap-beting-v-telegram-usloviia-igry-bonusy-i-aktsii-...
/2026/03/29/pinap-kz-aviator-usloviia-igry-bonusy-i-aktsii-...
/2026/03/30/obzor-pin-ap-kazino-ofitsialnyi-sait-s-aktualnoi-informatsiei-...
/2026/03/30/pobezhdai-s-novoi-igroi-v-pinap-i-vyigryvai-progressivnyi-dzhekpot
/2026/03/31/gde-bezopasno-khranit-svoi-uchetnye-dannye-dlia-vkhoda-v-pin-up
/2026/04/01/1xbet-app-i-get-1xbet-mobile-1xbet-apk-android-ios-i-1xbet-fi
/2026/04/08/aviator-oyna-v-qazan-rsmi-sayti-aviator-azerbaycan
```

These are **injected casino/sportsbook SEO spam** in German, Polish, Russian, and Azerbaijani — dated 2026-03-26 through 2026-04-08 (recent, ongoing). Translations: "Plinko casinos 2026", "Pin-Up casino review", "1xBet app download", "Aviator game Azerbaijan", etc. This is a classic Japanese/pharma-hack pattern adapted for gambling affiliate farms — someone has persistent write access to the prod WP database (compromised admin, vulnerable plugin, or a backdoor in the theme/uploads). **The fact that staging is clean strongly suggests the prod compromise is either database-side or via an auth vector that the staging container doesn't share.**

**This is urgent and independent of the DNS flip:**
- Do NOT flip DNS until you've confirmed the compromise vector, otherwise the attacker may re-inject into staging once it's live.
- On prod: `wp post list --post_type=post --post_status=publish --fields=ID,post_date,post_title,post_author | grep -iE 'plinko|pinap|aviator|1xbet|sportwetten|wetten|oasis|lugas'` — find the author account(s) used.
- Check `wp user list --role=administrator,editor,author,contributor` for unexpected accounts.
- `wp plugin list` and look for unknown or recently-modified plugins.
- Check `wp-content/uploads/` and theme files for PHP backdoors: `find wp-content -name "*.php" -newer /tmp/ref -mtime -60`.
- After cleanup: reset all admin passwords (you already have one pending), rotate secret keys in `wp-config.php`, and audit `wp_options` for injected `autoload` rows.

Also tied to this: prod has a `/category/casino` taxonomy term that staging doesn't. That's the category these spam posts are filed under — deleting the posts should cascade, but double-check.

**🟡 RED FLAG #2 — prod has 48 theme-demo pages in the sitemap:**

```
/add-listing /alert-box /bestsellers /blog /carousel /cart-2 /checkout-2
/checkout-2/checkout-pay /checkout-2/order-received /elements-boostrap
/featured-products /forms /gd-archive /gd-archive-item /gd-details
/goolge-map /home-2 /home-3 /home-4 /hot-deal-2 /icons /list-color
/location /my-account-2 /my-account-2/change-password /my-account-2/edit-my-address
/my-account-2/logout /my-account-2/lost-password /my-account-2/track-your-order
/my-account-2/view-order /on-sale /panel /pie-chart /products-by-sku
/recent-products /sale-products /sample-page /search /services /shop-2
/tab /table /testimonials /top-rated-products /typography /wctz
/wishlist-2 /woocommerce-to-zoho-inventory-plugin
```

These are **theme/plugin demo pages** that shipped with whatever premium theme is installed (`home-2/3/4`, `carousel`, `typography`, `alert-box`, `pie-chart`, `tab`, `table`, `icons`, `elements-boostrap` [sic — typo baked into theme]). Plus `my-account-2/*`, `cart-2`, `checkout-2`, `shop-2` which look like **duplicate commerce pages** from an aborted migration (the `-2` suffix is what WooCommerce does when you "install pages" twice). `sample-page` is WP's default. `woocommerce-to-zoho-inventory-plugin` and `wctz` look like plugin config pages that shouldn't be public.

**These should be deleted or noindexed before the flip** — they're dead weight in Google's index and make the site look unfinished. Staging is already cleaner (40 pages vs 79) because you've presumably been housekeeping there.

Note the 9 staging-only pages (`/change-password`, `/checkout-pay`, `/edit-my-address`, `/logout`, `/lost-password`, `/order-received`, `/track-your-order`, `/view-order`, `/products`) are the **cleaned-up versions** of the prod `my-account-2/*` and `checkout-2/*` sub-pages — these are the new canonical WooCommerce account/checkout endpoints from your earlier migration. Good. After the flip, you'll need 301s from the old `/my-account-2/*` and `/checkout-2/*` URLs to these clean slugs (you already planned cart/my-account redirects earlier in this doc — extend the list to cover all of these).

**🟢 Minor: staging-only extras**

- `/product/test-product-1` — test product, delete before flip
- `/shop` — WooCommerce shop page, prod doesn't have it in sitemap (probably marked noindex on prod or not assigned as the shop page); staging correctly surfaces it
- `/blog` — staging has it as a post URL (odd — probably because a post was created with that slug); prod has `/blog` as a page instead

**Author archive:** staging's Yoast sitemap exposes `/author/catfishinginfogmail-com` — that slug is the user's email address with `@` and `.` stripped, which is exactly the leak I warned about in the previous response. **Rename before the flip:** `wp user update <id> --user_nicename=steve-douglas` (or `catfishdude`).

**Bottom line and action order:**

1. **URGENT — investigate prod compromise.** The 16 spam posts are active malware, not just cruft. Don't flip DNS until the injection vector is closed, or you'll inherit the problem on staging.
2. **Delete the spam posts + `/category/casino`** from prod.
3. **Delete the 48 theme-demo pages** on prod (or at minimum noindex them in Yoast). Most of these are safe `wp post delete`s — none should have inbound traffic worth preserving.
4. **Rename Steve's user_nicename** on staging so the author sitemap doesn't leak the email-derived slug post-flip.
5. **Extend the redirect list** to cover `/my-account-2/*` → `/my-account/*`, `/checkout-2/*` → `/checkout/*`, `/shop-2` → `/shop` in addition to the cart/my-account redirects you already planned.
6. **Delete `/product/test-product-1`** on staging before flip.

All raw sitemap files are at `/tmp/mrh-sitemap/` on this host if you want to poke at them directly.
<!-- agent:boundary:34f8bdf5 -->
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- **Test Venmo button (if possible)** — ask a team member or tester to click the Venmo button and confirm approval flow; or check modsec audit log for zero 933120 hits on `form_encoded` when attempted. (blocked by: user password reset)
- **Rotate DB passwords** — `MARIADB_ROOT_PASSWORD` + `MARIADB_PASSWORD` still on the previous values in `docker-compose.yml` / `.env`; update `wp-config.php`, `docker compose up -d --force-recreate`.
- **Livestream VOD trim** — the Stripe settings dump segment still needs manual trim in post-production.
<!-- /agent:pending -->
