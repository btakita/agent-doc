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

**Yes.** Changing to "Regular Web Application" is a superset — it supports **all** grant types:

- **Password** (ROPG) — embedded login form (current implementation)
- **Authorization Code** — redirect-based flow (if you want Auth0 Universal Login later)
- **Implicit** — original SPA flow
- **Client Credentials** — M2M

Self-serve signup (`/dbconnections/signup`) works regardless of app type — it's a database connection API, not a grant type.

**One app handles both censible.co and workers.dev.** No need for separate applications.

What do I need to do again?
<!-- /agent:exchange -->
