# OWASP CRS narrow exclusions — template

**When to use:** OWASP ModSecurity CRS is blocking a legitimate request and you need to narrow the rule rather than disable it wholesale. Applies to Imunify360, ModSecurity on nginx, Plesk, cPanel, and any CRS 3.x deployment.

**Principle:** Never `SecRuleRemoveById` a rule globally to fix one endpoint. Narrow the target (parameter, URI path, or request method) so every other request on the host stays protected.

## Directive cheat sheet

| Goal | Directive | Scope |
|---|---|---|
| Remove a specific arg from a rule's target list | `SecRuleUpdateTargetById <id> "!ARGS:<name>"` | Global, parameter-scoped |
| Remove a specific cookie from a rule's target list | `SecRuleUpdateTargetById <id> "!REQUEST_COOKIES:<name>"` | Global, cookie-scoped |
| Remove a specific header | `SecRuleUpdateTargetById <id> "!REQUEST_HEADERS:<name>"` | Global, header-scoped |
| Disable a rule only on one URI path | `<LocationMatch "^/path">` + `SecRuleRemoveById <id>` | Path-scoped, full rule disable |
| Disable a rule only on one query string | `<If "%{QUERY_STRING} =~ /.../"">` + `SecRuleRemoveById <id>` | Query-string-scoped, full rule disable |
| Lower paranoia for one vhost | `<VirtualHost ...>` + `SecAction "id:90000,phase:1,nolog,pass,setvar:tx.paranoia_level=1"` | Vhost-scoped |

**Strongly prefer `SecRuleUpdateTargetById`** over `SecRuleRemoveById`. The former narrows surgically (one param); the latter disables the whole rule for everything matched by the wrapping location.

## Workflow: narrow a new false positive

### 1. Confirm it IS a false positive

```bash
ssh <server> 'sudo tail -f /var/log/httpd/error_log' | grep -E 'ModSecurity|REQUEST-9'
```

Reproduce the failing request. You want a log line like:

```
[id "<RULE_ID>"] ... [msg "<human description>"] ... Matched Data: <match> found within <target>:<name>
```

Extract:
- `RULE_ID` — e.g. `933120`
- `target` — `ARGS`, `REQUEST_COOKIES`, `REQUEST_HEADERS`, etc.
- `name` — e.g. `post_data`

If the match is in a parameter that legitimately contains the matched data, it's a false positive. If the match is in a parameter that *shouldn't* contain it, investigate the app before disabling anything.

### 2. Write the exclusion

The narrowest possible exclusion for a parameter false positive:

```apache
SecRuleUpdateTargetById <RULE_ID> "!<target>:<name>"
```

Example (the monsterrodholders fix):

```apache
SecRuleUpdateTargetById 933120 "!ARGS:post_data"
```

This removes `ARGS:post_data` from rule 933120's target list. All other args in all other requests are still matched by the rule.

### 3. Store the exclusion with context

Create (or append to) a local-rules file. **Always include a comment header** explaining *why* — so the next admin doesn't delete it during a routine cleanup.

**Apache path:** `/etc/httpd/modsecurity.d/local_rules/<app>-crs-exclusions.conf`

```apache
# <app> CRS exclusions
# Last updated: <YYYY-MM-DD> by <who>
#
# Rule <RULE_ID>: <rule name>
# Symptom: <one-line user-visible symptom>
# Why needed: <why the match is a false positive>
# Scope: <which target was excluded>
# Ticket / commit: <link>

SecRuleUpdateTargetById <RULE_ID> "!<target>:<name>"
```

**nginx path:** Inside the server block:
```nginx
modsecurity_rules '
# <same comment header>
SecRuleUpdateTargetById <RULE_ID> "!<target>:<name>"
';
```

### 4. Test and reload

```bash
# Apache
sudo apachectl configtest && sudo systemctl reload httpd

# nginx
sudo nginx -t && sudo systemctl reload nginx
```

Re-run the failing request and confirm it now succeeds. Tail the error log to confirm the rule ID no longer fires for this request (it may still fire for legitimate attacks, which is the point).

### 5. Document in consuming repo's SPEC.md

Add an entry to the consuming repo's `SPEC.md` under "Host mod_security" with:
- The rule ID and human name
- The symptom
- The exclusion directive
- Whether this pattern can recur elsewhere

## Known recurring false positives

Compiled from real incidents — check here before diagnosing from scratch.

### 933120 — `user_agent=` inside `post_data` or `form_encoded` (WooCommerce Order Attribution)

Two cases share this rule because both wrap the serialized checkout form in an arg that contains `wc_order_attribution_user_agent=`:

- **`ARGS:post_data`** — classic WC checkout (`application/x-www-form-urlencoded`)
- **`ARGS:form_encoded`** — PPCP smart buttons (PayPal / Venmo / Apple Pay / Google Pay; `application/json` body)

Always exclude **both** on any WooCommerce site that uses `woocommerce-paypal-payments` — fixing only `post_data` leaves the PPCP buttons broken (endpoints return `404` via Imunify360's `/.noindex.html` rewrite, which hides the underlying 403). Companion rule `931100` also needs the same two exclusions.

Full runbook: [waf-933120-order-attribution.md](waf-933120-order-attribution.md)

### 941* — XSS family inside rich-text editor content

Block editor content with inline `<script>` or `on*=` attributes can trip rules like `941100`, `941160`. If the false positive is in `ARGS:content`:

```apache
SecRuleUpdateTargetById 941100 "!ARGS:content"
SecRuleUpdateTargetById 941160 "!ARGS:content"
```

Prefer block editor sanitization over disabling — only exclude if you trust the editor role.

### 942* — SQL injection patterns inside post content or search terms

Search queries containing `'` or `--` can trip `942100`, `942130`. If the false positive is a real search box:

```apache
SecRuleUpdateTargetById 942100 "!ARGS:s"
```

(`s` is WordPress's default search query parameter.)

### 920350 — Host header contains invalid chars

Fires on IP-address Host headers (e.g. `127.0.0.1:8080` for backend health checks from the proxy). Usually fixed by excluding the backend IP:

```apache
<LocationMatch ".*">
    SecRule REMOTE_ADDR "@ipMatch 127.0.0.1" "id:99001,phase:1,nolog,pass,ctl:ruleRemoveById=920350"
</LocationMatch>
```

### 949110 — Inbound anomaly score exceeded

This is the **final denial rule**, not a match rule. If you see 949110 in the log, scroll up — the actual match is from a different rule in the same request. Fix the root match, don't exclude 949110.

## Anti-patterns

- **`SecRuleRemoveById 933120`** without a location scope → disables PHP injection protection for every arg on every request on the host
- **`SecRuleEngine Off`** inside a `<Location>` → disables the entire WAF for that location, not just one rule
- **Copy-pasting exclusions from Stack Overflow without understanding them** → attackers use the same rule IDs in their reconnaissance; a misapplied exclusion can silently open an injection path
- **Skipping the comment header** → next admin deletes the file during cleanup and reintroduces the bug
