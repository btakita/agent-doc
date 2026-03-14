---
agent_doc_session: fce94c2a-2072-49c6-924b-144e96b540b7
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---


The @rappstack/* packages were recently published.
The apps used submodules from rappstack.
Now that @rappstack/* packages are published, the apps need to reference the @rappstack/* packages instead of using submodules.

Convert brookebrodack-dev, cogov-dev, censible-core, drmanny-dev, herbaliciousbliss-dev, prescience-notes-dev

Use yalc if possible. And deploy via github actions. The deployments must depend on the published npm packages...not yalc. yalc is only for development.

<!-- agent:exchange -->

### Re: herbaliciousbliss-dev relysjs fix

You're right — the `relysjs/server` import was a leftover compatibility shim. `ctx/index.ts` imported `elysia_context__set` to set an elysia context for `@rappstack/domain--server--auth`, but that package doesn't use `elysia_context` at all.

**Fix:** Removed the `elysia_context__set` import and call. Now uses only `rhonojs/server`. Committed and pushed — CI triggered.

**Deploy status:**

| App | URL | CI |
|-----|-----|-----|
| briantakita.me-dev | briantakita.me | ✅ DEPLOYED |
| prescience-notes-dev | pn.briantakita.me | ✅ DEPLOYED |
| brookebrodack-dev | brookebrodack.net | ✅ DEPLOYED |
| cogov-dev | cogov.me | ✅ DEPLOYED |
| drmanny-dev | pe.briantakita.me | 🔄 CI running |
| herbaliciousbliss-dev | hb.briantakita.me | 🔄 CI running (relysjs shim removed) |

I see many errors for the drmanny-dev build. See https://github.com/btakita/drmanny-dev/actions/runs/23099026077/job/67096311943

herbaliciousbliss-dev has errors https://github.com/btakita/herbaliciousbliss-dev/actions/runs/23099060500/job/67096404135
<!-- /agent:exchange -->
