---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration complete ✓. All core features deployed. Minor UI polish gaps (executives, peers, contact info). Remaining setup: custom domains, Auth0 callback URLs.
<!-- /agent:status -->

<!-- agent:exchange -->

**Auth0 app compatibility:** A single Auth0 app can handle the Password Grant, but there's a type constraint:

- **SPA apps** (Single Page Web Applications) **cannot** use the Password grant type by default in Auth0
- **Regular Web Applications** **can** enable the Password grant

**Options:**
1. **Change your existing app type** to "Regular Web Application" in Auth0 dashboard → Settings → Application Type. Then enable Password under Advanced Settings → Grant Types. Same Client ID works.
2. **Use a separate app** — create a "Regular Web Application" with Password grant enabled, use its Client ID for the workers.dev deployment. Your original SPA app stays for censible.co if it uses a different auth flow.

Since censible.co and mi-www.site historically used different auth methods, you likely already have two Auth0 apps. Check your Auth0 dashboard — you probably have one "Regular Web Application" (for ROPG) and one SPA app. Use the Regular Web Application's Client ID.

**To verify:** In Auth0 → Applications, check which app has `XudvEP3pRwUXcXENXvZ7BXSQxS3zogqJ` — if it's an SPA, either change its type or use the other app's Client ID instead.

Did the original mi-www.site use auth0 or a different auth mechanism?
<!-- /agent:exchange -->
