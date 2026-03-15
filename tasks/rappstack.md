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

### Re: CI still failing

**Good news:** `rebuildjs@0.70.46` works — builds exit cleanly now (no more hanging).

**drmanny-dev:** `lockfile had changes, but lockfile is frozen` — regenerated and pushed. New CI running.

**herbaliciousbliss-dev:** Build exits cleanly, but prerender fails with `p is not a function` on `/`, `/lyra`, `/sitemap.xml`. `relement__use` is in both `index.server.ts` files but isn't resolving module duplication. Need to investigate the server bundle.

Want me to dig into herbaliciousbliss esbuild output?

Yes
<!-- /agent:exchange -->
