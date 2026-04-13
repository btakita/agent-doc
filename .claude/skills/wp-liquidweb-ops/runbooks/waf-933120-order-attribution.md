# WAF rule 933120 blocks WooCommerce Order Attribution

**One-line:** OWASP CRS 3.3.5 rule `933120` matches `wc_order_attribution_user_agent=...` inside a WC arg wrapping the serialized form, scores CRITICAL, trips rule `949110`, blocks every WC AJAX request. Fix = narrow target exclusion on that arg. **Two known arg wrappers:** `post_data` (classic checkout) and `form_encoded` (PPCP JSON body).

## Symptom

- WooCommerce checkout page shows `blockUI` spinner overlays on the order-review table + payment section that never clear
- Stripe UPE element never mounts (it's gated on the `updated_checkout` event, which requires a successful `update_order_review` response)
- Looks identical to a broken payment gateway — but the gateway is fine
- `jQuery.active === 0` (no AJAX in flight — the request already failed, silently)
- Works fine on local dev, breaks only behind the host WAF
- **PPCP variant:** clicking PayPal / Venmo / Apple Pay / Google Pay smart buttons shows "Something went wrong. Please try again or choose another payment source." — PPCP's `ppc-save-checkout-form` and `ppc-create-order` requests return `404` instead of `200` JSON

## The 404 red herring (Imunify360 / Apache behavior)

When ModSec + Imunify360 denies a request with 403, Apache rewrites the intercepted URL to `/.noindex.html` before serving the response. The backend has no such file, so the theme's 404 page (often 100+ KB of HTML) is returned. **What the browser/DevTools sees is an HTTP 404, not a 403.** The real 403 is only visible in `modsec_audit.log`.

Diagnostic signature:
- DevTools: `POST /?wc-ajax=...` → `404 Not Found`, response body is the theme's 404 HTML page (look for `<title>` containing "404" or "Not Found")
- Same request via `curl` direct to the backend (`127.0.0.1:8080`) → `200` with JSON
- `grep -E '933120|949110' /var/log/httpd/modsec_audit.log` → the actual match

**Don't chase the 404.** If direct-to-backend works but through-the-proxy returns 404, check `modsec_audit.log` for a 403 on the same request ID before touching PHP routing.

## Root cause

WooCommerce 8.5+ ships **Order Attribution** (on by default). It adds a hidden `wc-order-attribution-inputs` block to `form.checkout` whose fields are serialized into whatever arg wraps the checkout form on the current AJAX path. One field is literally named `wc_order_attribution_user_agent=<browser UA>`.

OWASP CRS rule **933120** (`REQUEST-933-APPLICATION-ATTACK-PHP.conf`, paranoia level 1) is "PHP Injection Attack: Configuration Directive Found." It matches `php.ini` directive names followed by `=` inside request args. `user_agent` is a php.ini directive, so any arg value containing `user_agent=<anything>` scores CRITICAL (+5). Rule **949110** then denies at score ≥ 5.

### Case 1: Classic checkout — arg name is `post_data`

Standard WC checkout posts `application/x-www-form-urlencoded` with the serialized form nested under `post_data`:

1. Browser POSTs `?wc-ajax=update_order_review` with `post_data=...&wc_order_attribution_user_agent=Mozilla...`
2. ModSecurity scores +5 on `ARGS:post_data`, `949110` denies with 403
3. Host Apache rewrites denied request to `/.noindex.html` → backend 404
4. WC's jQuery AJAX fails silently, never unblocks the UI, Stripe UPE never mounts

### Case 2: PPCP smart buttons — arg name is `form_encoded`

The `woocommerce-paypal-payments` plugin (PPCP) — which renders the **PayPal, Venmo, Apple Pay, and Google Pay** smart buttons — posts `application/json` with a different shape. ModSec's JSON body processor flattens the JSON into ARGS, so the inner URL-encoded form becomes `ARGS:form_encoded`:

```json
POST /?wc-ajax=ppc-save-checkout-form
Content-Type: application/json

{
  "nonce": "...",
  "form_encoded": "wc_order_attribution_source_type=typein&...wc_order_attribution_user_agent=Mozilla..."
}
```

1. ModSec scores +5 on `ARGS:form_encoded`, `949110` denies with 403
2. Apache rewrites to `/.noindex.html` → backend 404
3. Browser sees 404 on `ppc-save-checkout-form` and `ppc-create-order`
4. PPCP JS throws "Something went wrong. Please try again or choose another payment source."
5. **All four smart buttons (PayPal, Venmo, Apple Pay, Google Pay) are affected** — they share the same PPCP checkout API

If a `post_data` exclusion is already in place (Case 1 fix) but PPCP still breaks, it's because the arg name is different. Both exclusions are needed.

## Affected endpoints

### Classic checkout (`post_data` wrapper)

- `?wc-ajax=update_order_review` — fires on checkout page load (most visible break)
- `?wc-ajax=checkout` — order submission
- `?wc-ajax=update_shipping_method` — shipping selection changes
- `?wc-ajax=apply_coupon` / `remove_coupon`

### PPCP smart buttons (`form_encoded` wrapper)

- `?wc-ajax=ppc-save-checkout-form` — fires when user clicks any PPCP button
- `?wc-ajax=ppc-create-order` — PayPal/Venmo order creation
- Any other `ppc-*` JSON endpoint that ships the serialized form

**Both cases share rule 933120 and rule 949110** — the difference is purely the arg name.

## Fix

Add narrow target exclusions — removes **only** the affected arg name from rule 933120's target list. Every other argument in every other request is still fully protected. **Always add both `post_data` and `form_encoded` on any site using PPCP**; adding just one leaves the other checkout path broken.

### 931100 is a preemptive companion

Rule `931100` ("Possible Remote File Inclusion — URL Parameter using IP address") matches URL-shaped values in the same `wc_order_attribution_*` field set (e.g. `wc_order_attribution_referrer=https://...`). Once `933120` stops eating the request, `931100` fires next on the same arg. Add both in one pass to avoid a second round-trip.

**Apache + Imunify360 / InterWorx / Plesk / cPanel:**

```bash
ssh <server> "sudo tee /etc/httpd/modsecurity.d/local_rules/woocommerce-crs-exclusions.conf > /dev/null <<'EOF'
# WooCommerce CRS exclusions — 933120 / 931100 false positives.
#
# Rule 933120 ('PHP Injection: Configuration Directive') matches
# 'user_agent=' from wc_order_attribution_user_agent inside the arg
# that wraps the serialized checkout form. Two wrapper names in the wild:
#   - post_data    (classic WC checkout, application/x-www-form-urlencoded)
#   - form_encoded (PPCP smart buttons, application/json body)
#
# Rule 931100 ('RFI: URL Parameter using IP') matches the URL-shaped
# wc_order_attribution_referrer field in the same param. Preemptive — it
# fires the moment 933120 stops eating the request.
#
# Narrow exclusion: only these two args are exempted from these two rules.
# All other args in all other requests stay fully protected.
SecRuleUpdateTargetById 933120 \"!ARGS:post_data\"
SecRuleUpdateTargetById 933120 \"!ARGS:form_encoded\"
SecRuleUpdateTargetById 931100 \"!ARGS:post_data\"
SecRuleUpdateTargetById 931100 \"!ARGS:form_encoded\"
EOF
sudo apachectl configtest && sudo systemctl reload httpd"
```

**nginx + ModSecurity:**

Same directives go into the vhost's `modsecurity_rules` block or an included `.conf` under `/etc/nginx/modsec/`:

```nginx
modsecurity_rules '
SecRuleUpdateTargetById 933120 "!ARGS:post_data"
SecRuleUpdateTargetById 933120 "!ARGS:form_encoded"
SecRuleUpdateTargetById 931100 "!ARGS:post_data"
SecRuleUpdateTargetById 931100 "!ARGS:form_encoded"
';
```

Then `sudo nginx -t && sudo systemctl reload nginx`.

**Cloudflare WAF (managed rules):**

The OWASP managed ruleset contains the same CRS rule. Create a WAF exception in the Cloudflare dashboard → Security → WAF → Managed Rules → find rule `933120` → "Skip" for URI path matching `*wc-ajax=*`. (No directive file to edit — it's UI-driven.)

## Verify

After applying + reloading:

1. Browser: visit `/checkout/` on the affected host
2. DevTools Network tab: `update_order_review` XHR returns `200`, not `404`
3. DOM check:
   - `document.querySelectorAll('.blockUI').length === 0`
   - `document.querySelector('.wc-stripe-upe-element iframe')` exists
   - Place Order button is enabled
4. chromium-bridge equivalent: `{blocked: document.querySelectorAll('.blockUI').length, upeMounted: !!document.querySelector('.wc-stripe-upe-element iframe')}` → `{blocked: 0, upeMounted: true}`

## Rejected alternatives and why

- **`SecRuleRemoveById 933120` wholesale** — kills PHP-injection protection for every arg on every request. Too broad.
- **`SecRuleRemoveById 933120` inside `<LocationMatch "wc-ajax=update_order_review">`** — `LocationMatch` does not match query strings. Would need `<If "%{QUERY_STRING} =~ /.../">`, more fragile, and misses the other WC AJAX endpoints.
- **`add_filter('woocommerce_order_attribution_enabled', '__return_false')`** — disables WC Order Attribution entirely. Loses UTM source, referrer, and campaign tracking that are useful for ad-spend ROAS.
- **JS filter to strip `wc_order_attribution_user_agent` before submit** — fragile against WC updates, runs on the client.

## Where this can recur

This is not a monsterrodholders bug — it is a collision between a WC default and a CRS default. Expect it on:

- Any WooCommerce 8.5+ site behind OWASP CRS 3.3.5
- Imunify360 (CloudLinux default on most managed hosts)
- ModSecurity CRS 3.3.x on nginx
- Cloudflare WAF with OWASP managed rules enabled
- Plesk with ModSecurity enabled, cPanel with the default WAF bundle

**Diagnostic signature** (paste this into any new-site onboarding doc):
- Classic checkout: spinner + `jQuery.active === 0` + `grep 933120 /var/log/httpd/error_log` hit = this exact bug, paste the fix above.
- PPCP smart buttons: "Something went wrong…" dialog + 404 on `ppc-*` endpoints + `grep form_encoded /var/log/httpd/modsec_audit.log` hit on 933120 = same bug, different arg.

**Key insight for diagnosis:** if direct-to-backend `curl` returns `200` but through-the-proxy returns `404`, don't chase the 404. It's Imunify360's `/.noindex.html` rewrite hiding a 403. Go straight to `modsec_audit.log`.

## Related CRS rules in the same family

Rules `933100`–`933210` all use the OWASP "PHP config directive / PHP function" dictionary. If another plugin posts a field named like `allow_url_fopen`, `auto_prepend_file`, `include_path`, or `open_basedir` as a value inside a serialized param, a different 933* rule can fire. Same fix pattern (`SecRuleUpdateTargetById <id> "!ARGS:<param>"`), different rule ID. When diagnosing, grep `REQUEST-933` not just `933120`.
