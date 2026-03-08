---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** All features deployed ✓. Auth flow live with AUTH0_CLIENT_ID. Remaining: custom domains.
<!-- /agent:status -->

<!-- agent:exchange -->

Set `AUTH0_CLIENT_ID=XudvEP3pRwUXcXENXvZ7BXSQxS3zogqJ` as wrangler secret and added to `.dev.vars`. Deployed — auth flow is now live.

Test: https://censible-root-web.brian-takita.workers.dev/login

Callback URL mismatch
https://censible.us.auth0.com/authorize?response_type=token&client_id=XudvEP3pRwUXcXENXvZ7BXSQxS3zogqJ&redirect_uri=https%3A%2F%2Fcensible-root-web.brian-takita.workers.dev%2Fcallback&audience=https%3A%2F%2Fcensible.auth0.com%2Fapi%2Fv2%2F&scope=openid+profile+email&screen_hint=signup
<!-- /agent:exchange -->
