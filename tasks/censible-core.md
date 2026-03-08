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

**Callback URL mismatch fix:** This is an Auth0 dashboard setting, not a code issue. In Auth0:

1. **Applications** → your app → **Settings**
2. Add to **Allowed Callback URLs**: `https://censible-root-web.brian-takita.workers.dev/callback`
3. Add to **Allowed Logout URLs**: `https://censible-root-web.brian-takita.workers.dev/solutions`
4. Add to **Allowed Web Origins**: `https://censible-root-web.brian-takita.workers.dev`
5. **Save Changes**

Multiple URLs can be comma-separated if you have existing ones. After saving, the login redirect should work immediately.

Is the mi-www.site migration complete?
<!-- /agent:exchange -->
