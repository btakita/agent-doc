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

### Re: Latest CI failures

**drmanny-dev** — `fa_waves_sine.svg` and `peak-existence.mp4` missing. Fix agent working on it.

**herbaliciousbliss-dev** — `relement__use` IS in both `index.server.ts` files (confirmed commit `8dc5072` checked out). The `p is not a function` error persists — deeper esbuild module graph issue. The minified bundle creates separate module instances despite `relement__use`.

Both need deeper investigation. Want me to dig into herbaliciousbliss-dev's esbuild config?
Yes. Does the build work locally?
<!-- /agent:exchange -->
