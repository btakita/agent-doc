---
agent_doc_session: c547aab7-46a1-4af4-8e56-54e6261feceb
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
In WordPress, what is the difference between an Admin and a Super Admin.

Tell me about Liquid Web Wordpress Dedicated.
### Re: Admin vs Super Admin in WordPress

**Admin** is the highest role within a single WordPress site. An Admin can:
- Install/manage plugins and themes
- Add/remove users
- Edit all content
- Manage settings, menus, widgets

**Super Admin** only exists in **WordPress Multisite** (a network of WordPress sites managed from one installation). A Super Admin can:
- Create/delete sites in the network
- Install plugins and themes **network-wide**
- Manage all sites' users and settings
- Access the Network Admin dashboard
- Override per-site Admin decisions

Key distinction: An Admin controls one site. A Super Admin controls the entire multisite network.

### Re: Liquid Web WordPress Dedicated

Liquid Web WordPress Dedicated is a managed WordPress hosting platform by Liquid Web, aimed at businesses and agencies needing high performance and hands-off server management.

**Plans & Pricing:**

| Plan | Sites | Storage | Bandwidth | Price |
|------|-------|---------|-----------|-------|
| Personal | 1 | 20 GB SSD | 5 TB | $29/mo |
| Freelance | Up to 4 | 40 GB SSD | 5 TB | $69/mo |
| Professional | Up to 10 | 100 GB SSD | 5 TB | $99/mo |
| Business | Up to 25 | 150 GB SSD | 5 TB | $149/mo |

(Promotional pricing may start as low as $4-5/mo on annual plans.)

**Key Features:**
- 99.99% uptime guarantee
- NGINX + PHP 8+ + multi-layered caching + integrated CDN
- Staging environments and 30-day backups
- Free SSL certificates, malware monitoring, Solid Security Pro plugin
- US or Europe data center choice
- 24/7/365 WordPress expert support (59-second response guarantee)
- 30-day money-back guarantee

For dedicated servers specifically (bare metal, not shared WordPress), pricing starts at $149/mo.

**Sources:**
- [Liquid Web WordPress Hosting](https://www.liquidweb.com/wordpress-hosting/)
- [Liquid Web Managed WordPress](https://www.liquidweb.com/wordpress-hosting/managed-wordpress/)
- [Liquid Web Review - CyberNews](https://cybernews.com/best-web-hosting/liquid-web-review/)
  - US or Europe data center choice
  - 24/7/365 WordPress expert support (59-second response guarantee)
  - 30-day money-back guarantee

  For dedicated servers specifically (bare metal, not shared WordPress), pricing starts at $149/mo.

  Sources:
  - https://www.liquidweb.com/wordpress-hosting/
  - https://www.liquidweb.com/wordpress-hosting/managed-wordpress/
  - https://cybernews.com/best-web-hosting/liquid-web-review/

  Now writing back to the document.

  Read 1 file (ctrl+o to expand)

● Bash(agent-doc commit tasks/software/wordpress.md)
  ⎿  [perf] commit.staging (hash_object+update-index): 3ms
     [perf] commit.git_commit: 18ms
     [main ed0f7114] agent-doc(wordpress): 2026-04-03 12:21:39
     … +6 lines (ctrl+o to expand)

● Response written and committed. Both questions answered in the document — Admin vs Super Admin roles, and Liquid Web WordPress Dedicated plans/features/pricing.

✻ Baked for 1m 7s

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
❯ 
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  ⏵⏵ bypass permissions on (shift+tab to cycle)
<!-- agent:boundary:bbefba9b -->
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
<!-- /agent:pending -->
