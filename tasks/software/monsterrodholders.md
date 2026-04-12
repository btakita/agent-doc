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
<!-- /agent:exchange -->
## Pending / Not Built

<!-- agent:pending patch=replace -->
- **Retry /checkout → PayPal** — confirm PayPal order creation reaches the approval flow after the `form_encoded` exclusion. (user action)
- **Test Venmo button (if possible)** — ask a team member or tester to click the Venmo button and confirm approval flow; or check modsec audit log for zero 933120 hits on `form_encoded` when attempted. (blocked by: user password reset)
- **Retry /checkout with test product + Stripe** — confirm Free Shipping / card path still works (regression check after CRS reload). (user action)
- **Rotate DB passwords** — `MARIADB_ROOT_PASSWORD` + `MARIADB_PASSWORD` still on the previous values in `docker-compose.yml` / `.env`; update `wp-config.php`, `docker compose up -d --force-recreate`.
- **Livestream VOD trim** — the Stripe settings dump segment still needs manual trim in post-production.
- **Decide on real shipping strategy** — Free Shipping is fine for testing; when going live, add Flat Rate / carrier rates above it.
- **Bump PPCP plugin 3.4.1 → 4.0.2** — defer until checkout is confirmed working end-to-end.
<!-- /agent:pending -->
