---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration complete ✓. All features deployed at esg.briantakita.me. Auth: change Auth0 app type to Regular Web Application + enable Password grant.
<!-- /agent:status -->

<!-- agent:exchange -->

**Steps to enable auth login:**

1. Go to https://manage.auth0.com/dashboard/us/censible/applications
2. Click **Censible** (the SPA app)
3. **Settings** tab → scroll to **Application Type** → change from "Single Page Application" to **Regular Web Application**
4. Scroll down to **Advanced Setting** → **Grant Types** tab → check **Password** box
5. Click **Save Changes**

That's it. The login at https://censible-root-web.brian-takita.workers.dev/login should work immediately after.

Complete. Is all of censible completely moved over?
I don't think I have access to the censible.co dns. But we can create ~~censible.briantakita.me~~ a subdomain so I can demo the app.

Basically, censible.co should remain as is. It is already the censible.co website. And we should not have the same content. But the company track can have it's own subdomain. Do you have suggestions? Perhaps esg.briantakita.me?

`esg.briantakita.me` is a great choice — it clearly describes the app's purpose (ESG company tracking) without conflicting with censible.co.

Done! Custom domain updated and deployed:
- **https://censible-root-web.brian-takita.workers.dev** — workers.dev URL (still active)
- **https://esg.briantakita.me** — custom domain (live now)

The migration is complete. Summary of what's deployed:
- Full-text company search (D1 + FTS5)
- Company detail pages with ESG scores, controversies, executives
- XLSX export (summary + detailed 5-worksheet)
- 3 benchmark portfolios (URTH, ERTH, PXWEX)
- User portfolio management (create/edit/view/rebalance)
- Auth via embedded ROPG (login/signup/forgot password) — pending Auth0 app type change
- Custom domain at esg.briantakita.me

**Still needed from you:** Change the Auth0 app type (steps listed above) to enable login.

esg.briantakita.me should only be the company lookup app. censible.co already has the Censible company content.

Perhaps we can rebr
<!-- /agent:exchange -->
