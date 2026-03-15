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

### Re: Streaming errors

I can't access CI logs via `gh run view --log` while the run is in progress — GitHub's API only serves completed run logs. The run (23099251298) is still in_progress (9+ min). The background fix agent pushed "create public symlinks in CI and fix esbuild plugin order."

For the herbaliciousbliss-dev build hang — I was running `bun run build` which only shows output on completion/error. The build is hanging silently, not streaming errors.

Let me wait for the drmanny-dev run to complete and check. For herbaliciousbliss, I'll try capturing output differently.

The error should immediately end the build process.
<!-- /agent:exchange -->
