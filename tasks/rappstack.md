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

### Re: Local build test

**herbaliciousbliss-dev build hangs locally.** `bun build.ts` runs indefinitely (tested with 120s timeout) — no output, no error, just hangs. briantakita.me-dev builds fine in ~30s with the same `rhonojs` v0.1.1.

The hang is NOT the `rhonojs__ready__wait` (that's already skipped). Something in the esbuild build itself hangs — likely the `esmcss_esbuild_plugin` or `rebuild_tailwind_plugin` during CSS processing.

**drmanny-dev** fix agent is still working on the missing files.

Investigating the herbaliciousbliss build hang now.

Errors are streaming. Why are you not detecting the streaming errors?
The process has not ended by there are many errors.
https://github.com/btakita/drmanny-dev/actions/runs/23099251298/job/67096897494
<!-- /agent:exchange -->
