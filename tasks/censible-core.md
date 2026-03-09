---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration complete ✓. Rebranded to "ESG Tracker" white-label. Deployed at esg.briantakita.me. Auth: change Auth0 app type to enable login.
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

Perhaps we can rebrand esg.briantakita.me. Censible did not want to take on the liability of having the company lookup app.

Understood — since Censible doesn't want the liability, we can rebrand. The subdomain and all app branding (header, page titles, etc.) can be changed easily. What name/brand would you like to use instead of "Censible"?

esg? Make it look like a white label sample app.

I basically want SEO and a license deal from this site. I may also use it to funnel business to martin-investments. I will need to reach out to Mary Ellen about driving traffic to martin-investments.

Done — rebranded to "ESG Tracker" and deployed. All user-visible "Censible" references replaced across 13 page files. Internal names (localStorage keys, DB fields, CSS classes) preserved.

Live at **https://esg.briantakita.me**

For the SEO + licensing + martin-investments funnel strategy:
- The white-label positioning is good for licensing — shows the platform is brandable
- For SEO, the ESG company/fund pages already have structured content (company names, tickers, ESG scores, industry data) that should index well
- When ready to funnel to martin-investments, we can add a CTA banner or "Powered by" link on the site
- Reach out to Mary Ellen when ready to discuss the traffic partnership

Deploy
<!-- /agent:exchange -->
