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

### Re: Investigating herbaliciousbliss + drmanny lockfile

**herbaliciousbliss-dev:** Deep investigation agent launched — analyzing the esbuild server bundle to find why `relement__use` isn't effective despite being present.

**drmanny-dev:** Lockfile still failing because `package.json` had uncommitted changes (background agent added `rebuildjs` as root dep). Fixed — committed both `package.json` + `bun.lock` and pushed. CI triggered.

Will update when both resolve.

drmanny-dev failed

```
0s
Run echo "Checking dist/browser/ for expected HTML files..."
Checking dist/browser/ for expected HTML files...
MISSING: app/peakexistence-site/dist/browser/index.html
Error: Process completed with exit code 1.
```

herbaliciousbliss-dev also failed:
```
Run cd app/herbaliciousbliss-site
$ bun build.ts
server started on port 4102
[static_export] using external server: http://localhost:4102
[static_export] site_url: https://hb.briantakita.me
http://localhost:4102/sitemap.xml p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn) TypeError: p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:27281)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:11209)
http://localhost:4102/ p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn) TypeError: p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:27281)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:11209)
[static_export] / -> 500 Internal Server Error
http://localhost:4102/lyra p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn) TypeError: p is not a function. (In 'p(o, () => i(a + 1))', 'p' is an instance of Fn)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:27281)
    at i (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:652)
    at <anonymous> (/home/runner/work/herbaliciousbliss-dev/herbaliciousbliss-dev/app/herbaliciousbliss-site/dist/server/index.js:1:11209)
[static_export] /lyra -> 500 Internal Server Error
[static_export] completed with 2 error(s)
2 errors: [ "/", "/lyra" ]
Exported 0 files
Error: Process completed with exit code 1.

```
<!-- /agent:exchange -->
