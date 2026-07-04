---
name: WordPress WAF / checkout ops → wp-liquidweb-ops skill
description: Pointer to the wp-liquidweb-ops skill for WordPress + WooCommerce managed-host ops (CRS exclusions, checkout diagnosis, mod_proxy ordering). Extracted from monsterrodholders 933120 incident 2026-04-12.
type: reference
originSessionId: 6d004033-6575-48c9-9ee2-3f612362bca7
---
**Use the skill, not this memory.** Content that used to live inline here was extracted to `.claude/skills/wp-liquidweb-ops/` on 2026-04-12 so multiple sites can share it.

**Where to look:**
- **`agent-loop/.claude/skills/wp-liquidweb-ops/SKILL.md`** — decision tree: "checkout broken? WAF false positive? wp-cli OOM? RedirectMatch no-op?"
- **`.../runbooks/checkout-spinner-diagnose.md`** — triage playbook, starts at "is JS firing?" ends at the specific rule ID or plugin cause
- **`.../runbooks/waf-933120-order-attribution.md`** — the exact fix for the WC Order Attribution + CRS 933120 collision that hit monsterrodholders on 2026-04-12
- **`.../runbooks/waf-crs-exclusions.md`** — generic template for narrowing ANY CRS rule with `SecRuleUpdateTargetById` instead of disabling wholesale; includes known-recurring false positives for 941*, 942*, 920350

**Trigger phrases that should send future-me to the skill:**
- "WC checkout spinner never clears"
- "Stripe UPE not mounting"
- "ModSecurity / CRS / Imunify blocking AJAX"
- "WordPress on Liquidweb / InterWorx / Plesk / cPanel"
- "WAF false positive on a WordPress site"
- "`id "933120"` in Apache error log"

**Site-specific details** (IPs, SSH aliases, page IDs, plugin lists) live in the consuming repo's `SPEC.md` (e.g. `tasks/software/monsterrodholders/SPEC.md`), never in the skill.

**Extraction plan:** When a second Liquidweb/CRS site lands, move the skill to `src/wp-liquidweb-ops/` and reference via `.claude/settings.json` `skillPaths`. Runbooks are already written to read identifiers from `SPEC.md`, so extraction is `git mv` + README edit.
