---
agent_doc_session: 159f6a0e-f91f-4485-87e2-7e85474bb00d
tmux_session: '0'
agent_doc_mode: template
---

# cogov.me

<!-- agent:status -->
**Status:** All tasks complete. demo.cogov.me live at https://demo.cogov.me
<!-- /agent:status -->

<!-- agent:exchange -->
### demo.cogov.me deployed

**https://demo.cogov.me** — live on CF Workers with custom domain.

Setup:
- Created D1 database `cogov-demo` (region ENAM)
- Initialized schema (11 tables)
- Deployed worker with custom domain route
- Switched rhonojs dependency from yalc to npm `^0.1.0`
- Committed + pushed to cogov-dev

**All pending tasks complete:**
- [x] herbaliciousbliss → rhonojs migration
- [x] Commit all uncommitted work across repos
- [x] Publish rhonojs to npm
- [x] Deploy demo.cogov.me to CF Workers
<!-- /agent:exchange -->
