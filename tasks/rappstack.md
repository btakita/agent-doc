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

### Re: rebuildjs fix — watch: false now uses build()

**Published `rebuildjs@0.70.46`:**
- `watch: false` → `esbuild.build()` (one-shot, exits on errors)
- `watch: true` → `esbuild.context().watch()` (existing behavior)
- All 56 rebuildjs tests pass
- herbaliciousbliss-dev build completes without hanging

**Updated both apps:**
- herbaliciousbliss-dev → `rebuildjs@0.70.46`, pushed, CI triggered
- drmanny-dev → `rebuildjs@0.70.46`, pushed, CI triggered

Both CIs should now succeed or fail cleanly — no more hanging.

The same error keeps on repeating. How do we get the process to stop once there is an error on CI?
<!-- /agent:exchange -->
