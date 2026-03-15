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

### Re: Video uploaded + CI retriggered

**drmanny-dev:** Uploaded `peak-existence.mp4` (251 MB) to R2. Retriggered CI (run 23099460179). Asset download will now include the video.

**herbaliciousbliss-dev:** Build hangs locally. The `p is not a function` prerender error in CI happens because the build produces broken output. Investigating.

**Re: "error should immediately end the build"** — The esbuild errors stream but don't terminate because `rebuildjs` uses `context().rebuild()` internally (even with `watch: false`), which doesn't auto-exit on errors like `build()` would.

Let's make `watch: false` use build instead of rebuild.
<!-- /agent:exchange -->
