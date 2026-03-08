---
agent_doc_session: 89519638-4a4e-4cd7-a210-47968e8c5185
tmux_session: '0'
agent_doc_mode: template
---

Convert censible-core to use rappstack with rhonojs.

<!-- agent:status -->
**Status:** All core features complete ✓. Benchmark portfolios ✓. mi-web/esg-web deployed ✓. Auth flow ✓. User portfolios CRUD ✓. Remaining: custom domains (need domain names), AUTH0_CLIENT_ID secret.
<!-- /agent:status -->

<!-- agent:exchange -->

**Getting AUTH0_CLIENT_ID:**

1. Go to https://manage.auth0.com → **Applications** → **Applications**
2. Find or create your SPA application (e.g. "Censible Web")
3. The **Client ID** is at the top of the app's **Settings** tab

If creating new: **+ Create Application** → "Censible Web" → **Single Page Web Applications**, then set:
- **Allowed Callback URLs**: `https://censible-root-web.brian-takita.workers.dev/callback`
- **Allowed Logout URLs**: `https://censible-root-web.brian-takita.workers.dev/solutions`
- **Allowed Web Origins**: `https://censible-root-web.brian-takita.workers.dev`

Then run:
```bash
cd src/censible-core/apps/root-web
npx wrangler secret put AUTH0_CLIENT_ID
# paste the client ID when prompted
```

Your Auth0 tenant: `censible.us.auth0.com`, audience: `https://censible.auth0.com/api/v2/`

I added AUTH0_CLIENT_ID to .env
<!-- /agent:exchange -->
