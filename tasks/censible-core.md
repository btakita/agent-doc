---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** Migration complete ✓. All features deployed. Auth: change Auth0 app type to Regular Web Application + enable Password grant. Remaining: custom domains.
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
I don't think I have access to the censible.co dns. But we can create censible.briantakita.me so I can demo the app.

I will discuss w
<!-- /agent:exchange -->
