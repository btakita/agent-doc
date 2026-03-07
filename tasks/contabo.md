---
agent_doc_session: a2267a78-5840-45a5-a654-ac0aa081b7d4
tmux_session: '0'
---

What websites are currently running on my Contabo account?

## Assistant

**Contabo VPS (207.244.234.186) — 5 running websites + Traefik reverse proxy:**

| Container | Domain | Port | Image |
|-----------|--------|------|-------|
| `cogov-www-1` | `cogov.me` | 4020 | cogov-www |
| `herbaliciosbliss-www-1` | `hb.briantakita.me` | 4102 | herbaliciosbliss-www |
| `peakexistence-www-1` | `pe.briantakita.me` | 4103 | peakexistence-www |
| `prescience-notes-www-1` | `pn.briantakita.me` | 4104 | prescience-notes-www |
| `mi-www_site-1` | `mi-www.site` | 4009 | mi-www_site + Redis |
| `traefik` | (reverse proxy) | 80/443 | traefik:beaufort |

All are behind Traefik with ACME TLS (Let's Encrypt). All have HTTPS redirect middleware enabled.

**Already migrated to Cloudflare Pages (no longer on Contabo):**
- `briantakita.me`
- `brookebrodack.net` (decommissioned today)

**Repos on VPS but not running containers:**
`brookebrodack-dev`, `briantakita.me-dev`, `censible-core`, `drmanny-dev`, `sish`, `tmux-config`, `yay`

## User

| `cogov-www-1` | `cogov.me` | 4020 | cogov-www |
| `herbaliciosbliss-www-1` | `hb.briantakita.me` | 4102 | herbaliciosbliss-www |
| `peakexistence-www-1` | `pe.briantakita.me` | 4103 | peakexistence-www |
| `prescience-notes-www-1` | `pn.briantakita.me` | 4104 | prescience-notes-www |
| `mi-www_site-1` | `mi-www.site` | 4009 | mi-www_site + Redis |

Please add these repos as submodules in agent-loop. Let's begin the work of moving the host to Cloudflare.
