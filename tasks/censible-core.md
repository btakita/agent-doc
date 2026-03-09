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

From the screenshot, the "Censible" app (`XudvEP3pRwUXcXENXvZ7BXSQxS3zogqJ`) is a **Single Page Application** — this is the one we're using but it **cannot use Password grant**.

**Fix:** Click on "Censible" → **Settings** → change **Application Type** to **Regular Web Application** → then **Advanced Settings** → **Grant Types** → enable **Password** → **Save Changes**.

Same Client ID stays, no code changes needed. The ROPG login form will work after this change.

Will the other type of self-serve signup/login work as well for censible.co?

(The "Auth0 Management API" app is Machine-to-Machine for backend API access — don't touch that one.)
<!-- /agent:exchange -->
