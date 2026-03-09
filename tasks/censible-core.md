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

### Auth Updated to Embedded Password Grant ✓

Switched from Auth0 Universal Login (redirect-based) to the **Resource Owner Password Grant** flow matching mi-www.site:

**What changed:**
- Login form embedded directly in `/login` page (no redirect to Auth0)
- Email + password entered on-site, POST to Auth0 `/oauth/token` with `grant_type: password-realm`
- Signup via Auth0 `/dbconnections/signup` (auto-login after)
- Forgot password via `/dbconnections/change_password`
- Removed `/callback` route (no longer needed)
- Token stored in `localStorage` same as before

**Auth0 dashboard requirement:** The Auth0 application must have **Password** grant type enabled:
1. **Applications** → your app → **Settings** → **Advanced Settings** → **Grant Types**
2. Enable **Password** grant type
3. Save

Does this support both logins or do I need another Auth0 app with this login?

No callback URL configuration needed anymore since there are no redirects.

**Test:** https://censible-root-web.brian-takita.workers.dev/login
<!-- /agent:exchange -->
