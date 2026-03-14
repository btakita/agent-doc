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

### Re: CI errors investigation

**drmanny-dev (run 23099026077):** Still in progress (9+ min). The previous run (23098920375) failed with:
- `tailwindcss` not resolved — **fixed** in current run (`conditions: ['style']`)
- `peak-existence.mp4` not found — video file not in git, needs R2 or exclude
- `fa_waves_sine.svg` not found — icon file missing

**herbaliciousbliss-dev (run 23099060500):** Build succeeds but **prerender fails**:
```
p is not a function (500 on / and /lyra)
```
This is the **relementjs state duplication** issue — tag factories not initialized in minified server bundle. Fix: add `relement__use(server__relement)` to server entry point.

**Next steps:**
1. drmanny-dev: handle missing `fa_waves_sine.svg` + `peak-existence.mp4`
2. herbaliciousbliss-dev: add `relement__use(server__relement)` to server entry

Want me to fix both?

Yes
<!-- /agent:exchange -->
