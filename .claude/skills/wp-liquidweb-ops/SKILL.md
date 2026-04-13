---
description: WordPress + WooCommerce operations on Liquidweb / InterWorx / Imunify360 (OWASP CRS 3.3.5) and similar ModSecurity-fronted managed hosts. Diagnose checkout breakage, narrow CRS exclusions, handle mod_proxy vs mod_rewrite ordering, wp-cli memory limits.
user-invocable: false
---

# wp-liquidweb-ops

Shared knowledge for operating WordPress + WooCommerce sites on Liquidweb-style managed hosts where the common denominator is:

- Host Apache (or nginx) reverse-proxying into a Docker WordPress container
- OWASP ModSecurity Core Rule Set 3.3.x (Imunify360, Plesk, cPanel, Cloudflare WAF managed rules)
- wp-cli run inside the container via `docker compose run`

Site-specific details (IPs, paths, plugin list, SSH alias) live in the **consuming repo's `SPEC.md`** — runbooks read from there or take it as an argument. Never hardcode site identifiers in this skill.

## When to invoke

| Symptom | Runbook |
|---|---|
| WooCommerce checkout spinner never clears, Stripe/UPE never mounts | [runbooks/checkout-spinner-diagnose.md](runbooks/checkout-spinner-diagnose.md) |
| WC AJAX returns 404 only on this host, works on local | [runbooks/waf-933120-order-attribution.md](runbooks/waf-933120-order-attribution.md) |
| Any OWASP CRS rule blocking a legitimate request — need to narrow, not disable | [runbooks/waf-crs-exclusions.md](runbooks/waf-crs-exclusions.md) |
| `wp` CLI hits "allowed memory size exhausted" but the site runs fine | [runbooks/wp-cli-memory-limit.md](runbooks/wp-cli-memory-limit.md) |
| `RedirectMatch` / `Redirect` in a vhost silently does nothing | [runbooks/apache-proxy-rewrite-order.md](runbooks/apache-proxy-rewrite-order.md) |

## Decision tree — WooCommerce checkout broken

```
checkout page loads 200?
├── no  → different problem, not this skill
└── yes
    ├── spinner overlays never clear + jQuery.active === 0?
    │   → checkout-spinner-diagnose.md
    │     → if error log shows id "933120"
    │       → waf-933120-order-attribution.md
    │     → if error log shows any other REQUEST-933 / REQUEST-941 / REQUEST-949
    │       → waf-crs-exclusions.md
    └── Stripe UPE iframe mounts but place-order 500s
        → not this skill (gateway credential / webhook issue)
```

## Scope — what belongs here vs. elsewhere

**Belongs in this skill** (transferable across sites):
- CRS rule IDs and their known false-positive patterns
- WC AJAX endpoint list and the shared `post_data` serialization
- mod_proxy vs mod_rewrite phase-ordering gotchas
- wp-cli SAPI vs FPM `php.ini` split
- ModSecurity directive syntax for narrow exclusions

**Does NOT belong here** (lives in consuming repo's `SPEC.md`):
- Server IPs, SSH aliases, hostnames
- Page IDs, plugin lists, option values
- Secret key storage paths (`passage` paths)
- Specific vhost filenames

## Extraction path

Currently lives at `agent-loop/.claude/skills/wp-liquidweb-ops/` for monsterrodholders (N=1 site). When a second Liquidweb/InterWorx/CRS site lands, extract to `src/wp-liquidweb-ops/` as its own submodule and reference via `.claude/settings.json` `skillPaths` from each consuming site repo. Because runbooks read identifiers from `SPEC.md`, extraction is `git mv` + README edit, not a rewrite.

## Related

- Consuming repo inventory: `tasks/software/monsterrodholders/SPEC.md` (monsterrodholders)
- Cloudflare WAF equivalents: same CRS dictionary, same fixes — directives live in managed rules UI instead of a `.conf` file
