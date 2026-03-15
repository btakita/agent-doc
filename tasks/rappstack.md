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

### Re: Both CI failures analyzed

**drmanny-dev:** Build "succeeds" (both browser/server say "done") but `dist/server/index.js` is missing. The `rebuildjs@0.70.46` change to use `esbuild.build()` produces no output — the outdir/outfile config isn't being passed correctly to `build()` vs the `context()` setup. This is a bug in the rebuildjs change. Need to fix the `build()` call to pass the same config as `context()`.

**herbaliciousbliss-dev:** Same prerender error. Background investigation agent still running.

**Root issue:** The `rebuildjs@0.70.46` `build()` path needs to replicate the same esbuild config that `context()` uses (especially `outdir`, `write`, `metafile`). The `context()` path builds this config internally in `rebuildjs` before passing to esbuild. The `build()` path needs the same config object.

Investigating the rebuildjs internals to fix the `build()` config.

The css and js need to be minimized when deployed to cloudflare.
<!-- /agent:exchange -->
