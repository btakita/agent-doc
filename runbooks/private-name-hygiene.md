# Private Name Hygiene

This `agent-loop` checkout is private and may contain customer/client project
names. Public or shared subrepos must not.

Before editing or committing under `src/agent-doc/`, `src/lazily-rs/`,
`src/tsift/`, or any repo intended to be published, do not copy private
customer names, domains, host aliases, task paths, fixture names, or acronyms
into code, tests, specs, docs, release notes, or commit messages.

Use anonymized names such as `sample-app`, `sample-portal`, `sampleorders.md`,
`sample-ssh-host`, `tasks/professional/sampleportal.md`, and
`src/sample-app/tasks/sampleorders.md`.

For `src/agent-doc`, keep a local denylist at:

```text
src/agent-doc/.agent-doc-private-denylist
```

That file is intentionally ignored by the `agent-doc` repo. Run this from
`src/agent-doc` before completing work there:

```bash
rg -n -f .agent-doc-private-denylist --glob '!target/**' .
```

The local denylist should include private markers such as:

- `equityfundingsource`, `Equity Funding Source`, `EFS`
- `monsterrodholders`, `Monster Rod Holders`, `MRH`
- `boost-client`

If a private name is needed to explain why a bug was found, keep that detail in
this private repo or the session document and use a neutral fixture name in the
target codebase.
