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

### Re: pe.briantakita.me styling

**herbaliciousbliss-dev: ✅ DEPLOYED** — CSS fix worked, CI passed.

**drmanny-dev: ❌ prerender fails** with `c.get is not a function` — same module duplication issue as herbaliciousbliss-dev had. Fix agent launched — applying `server_external_()` merge (root + local node_modules) to prevent duplicate bundling.

Fixing now.
<!-- /agent:exchange -->
