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

### Re: Fixing both

**2 fix agents launched:**

1. **drmanny-dev** — resolving missing `peak-existence.mp4` and `fa_waves_sine.svg`
2. **herbaliciousbliss-dev** — adding `relement__use(server__relement)` to server entry points

Will update with results.

The workflows have failed. See the errors.
<!-- /agent:exchange -->
