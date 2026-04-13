# WooCommerce checkout spinner diagnostic playbook

**When to use:** Checkout page loads (HTTP 200) but `blockUI` spinner overlays never clear and the payment element (Stripe UPE / PayPal / etc.) never mounts. Looks like a broken gateway, usually isn't.

**Philosophy:** When a WordPress front-end action silently fails with "JS looks broken but isn't," debug the host stack before the plugin stack. On a managed host behind a WAF, the WAF is the single most common silent break — ahead of plugin bugs, theme conflicts, or gateway issues.

## Triage order

Work top-to-bottom. Stop at the first step that explains the failure.

### 1. Is JS actually firing?

In the browser DevTools console on the checkout page:

```javascript
jQuery.active    // number of in-flight AJAX requests
document.querySelectorAll('.blockUI').length   // count of spinner overlays
```

- `jQuery.active > 0` → AJAX is in-flight, wait 5s and re-check. If still > 0, the backend is hung (FPM pool full, DB lock) — not this runbook.
- `jQuery.active === 0` AND `blockUI.length > 0` → **AJAX already failed silently and didn't unblock the UI.** This is the bug. Continue.

### 2. Is there a failed XHR in the Network tab?

Open DevTools Network tab, reload `/checkout/`, filter to `wc-ajax`. Look for the `update_order_review` request.

- **Status 404** — request was blocked upstream and rewritten to a not-found path. Almost always a WAF. → jump to step 4.
- **Status 403** — WAF blocked and did NOT rewrite. → jump to step 4.
- **Status 500** — backend error. Check `wp-content/debug.log` and `docker compose logs wordpress`.
- **Status 200 but response is HTML not JSON** — a plugin is hijacking the output (often a theme function running on `init`). `docker compose logs wordpress` and grep for PHP warnings/notices.
- **Request doesn't fire at all** — JS error earlier in the page broke WC's init. Check the Console tab for uncaught errors.

### 3. chromium-bridge equivalent (headless)

If you don't have a browser open, use chromium-bridge to get the same state:

```javascript
// Run via chromium-bridge evaluate on the checkout page after 3s settle:
({
  blocked: document.querySelectorAll('.blockUI').length,
  jQueryActive: jQuery.active,
  stripeIframe: document.querySelectorAll('.wc-stripe-upe-element iframe').length,
  upeChildren: document.querySelector('.wc-stripe-upe-element')?.children.length ?? 0,
  placeOrderEnabled: !document.querySelector('#place_order')?.disabled,
  payMethod: document.querySelector('input[name="payment_method"]:checked')?.value ?? null,
})
```

Healthy checkout:
```json
{"blocked": 0, "jQueryActive": 0, "stripeIframe": 1, "upeChildren": 1, "placeOrderEnabled": true, "payMethod": "stripe"}
```

Broken by CRS 933120:
```json
{"blocked": 2, "jQueryActive": 0, "stripeIframe": 0, "upeChildren": 0, "placeOrderEnabled": false, "payMethod": "stripe"}
```

### 4. Check the WAF error log

SSH to the server and tail the error log while reloading the checkout page:

```bash
ssh <server> 'sudo tail -f /var/log/httpd/error_log' | grep -E 'ModSecurity|REQUEST-9'
```

(nginx: `/var/log/nginx/error.log`. Cloudflare: Security → Events in the dashboard.)

Reload `/checkout/` in the browser. Look for lines matching:

```
[id "933120"] ... [msg "PHP Injection Attack: Configuration Directive Found"] ... Matched Data: = found within ARGS:post_data
```

- **Rule 933120 match** → this is the WC Order Attribution bug. → [waf-933120-order-attribution.md](waf-933120-order-attribution.md)
- **Any other REQUEST-933 / REQUEST-941 / REQUEST-942 / REQUEST-949 match** → CRS false positive on a different rule. → [waf-crs-exclusions.md](waf-crs-exclusions.md)
- **No ModSecurity lines for the time window** → WAF is not involved. Back to step 2 on the XHR status.

### 5. Bypass the proxy to isolate backend vs. WAF

If the error log is inconclusive, hit the Docker backend directly (bypassing host Apache + WAF):

```bash
ssh <server> 'curl -sk -o /dev/null -w "%{http_code}\n" \
  -H "Host: <site-hostname>" \
  -X POST "http://127.0.0.1:8080/?wc-ajax=update_order_review" \
  --data "post_data=wc_order_attribution_user_agent%3DMozilla%2F5.0"'
```

- **200** from the backend but **404/403** through the proxy → host WAF is the culprit. Re-check step 4.
- **500/404** from the backend directly → it's a WP / plugin / DB issue, not the WAF. Check `docker compose logs wordpress`.

### 6. Common plugin-side causes (after ruling out WAF)

Only after steps 1–5 fail to find a WAF hit:

- **Payment gateway credentials missing or invalid** — check `woocommerce_<gateway>_settings` options
- **`woocommerce-paypal-payments` active with empty credentials** — deactivate it or provide creds (this plugin bricks checkout client-side if enabled without config)
- **Theme `functions.php` hooking `woocommerce_update_order_review_fragments`** — broken fragment return value
- **Caching plugin** serving cached checkout with stale nonce — clear cache, confirm no caching on `/checkout/`

## Anti-pattern: diagnosing from symptoms

Don't claim the cause before testing a live checkout in a real browser. The DOM state (`blocked`, `jQueryActive`, `stripeIframe`) is the ground truth. PPCP-without-credentials, theme hook conflicts, and WAF blocks all present identically until you look at the Network tab and the error log.
