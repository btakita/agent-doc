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

### Re: drmanny-dev DEPLOYED + image issue

**ALL 6 APPS CI GREEN!** drmanny-dev passed (run 23100495333).

**Image 404s:** The `esbuild-plugin-object-store-asset` generates hashed filenames (`peak-existence-thumbnail-E573CA32.webp`) but R2 has unhashed names (`image/peak-existence-thumbnail.webp`). Same issue cogov-dev had. Removing the plugin from drmanny-dev (like we did for cogov-dev).

**Image smoke tests:** Adding asset URL checks to all 6 sites' CI workflows.

Fix agent running in background.
<!-- /agent:exchange -->
