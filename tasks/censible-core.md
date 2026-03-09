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

**Yes, mi-www.site used Auth0** — specifically the Resource Owner Password Grant (ROPG) flow, confirmed from source:

- `libs/ui-solid/src/auth0/LoginForm_auth0.tsx` — embedded email/password form, POSTs to `https://{AUTH0_DOMAIN}/oauth/token`
- `libs/ui-solid/src/auth0/login.ts` — uses `grant_type: password-realm`, `realm: Username-Password-Authentication`
- `libs/ui-solid/src/auth0/SignupForm_auth0.tsx` — embedded signup
- `libs/ui-solid/src/auth0/Auth0ForgotPasswordForm.tsx` — forgot password
- All via `@ctx-core/auth0` library

The new implementation matches this exactly — same Auth0 ROPG flow, same realm, same embedded forms. So you can use the same Auth0 application that mi-www.site used. Just set that app's Client ID as `AUTH0_CLIENT_ID`.

Which app did it go to? See screenshot.
<!-- /agent:exchange -->
