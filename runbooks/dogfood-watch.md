# Dogfood Watch — live agent-doc sessions → backlog + queue

`bin/agent-doc-dogfood-watch` turns issues in running agent-doc sessions into
tracked work automatically. It is the dogfooding feedback loop for this repo:
several long-lived supervisors run against different documents, and defects that
show up in their logs used to be found only when a human happened to read
`ops.log`.

## What it does

1. Enumerates every live `agent-doc start` supervisor (`ps` + `/proc/<pid>/cwd`,
   so relative document paths under submodules resolve correctly). `--doc PATH`
   narrows a one-shot invocation to explicit sources. Every discovered document
   is considered; the typed opt-in in step 2 is the only filing gate.
2. Resolves the binary-owned typed policy with `agent-doc dogfood policy <FILE>
   --json`. Absent/false is off. Session frontmatter overrides `.agent-doc/project.md`;
   `true` targets the current document, a string is project-relative, and the map
   form carries optional `path` plus explicit `auto`.
3. Resolves each document's project root (nearest `.agent-doc/`) and asks the
   binary to classify that project's log: `agent-doc ops summary --json
   --project-root <root>`. Supervisor panics/aborts/OOM in
   `.agent-doc/logs/supervisor-stderr.log` are matched separately, since
   `ops summary` does not cover that file.
4. Aggregates clusters by **defect signature** — family plus an example line with
   session/cycle/pid/hash noise stripped — so one defect hitting four projects
   files one item naming all four, not four duplicates.
5. Files each new signature with `agent-doc backlog <target> add "id=dfw<hash> ..."`.
   `auto: false` files backlog only. `auto: true` adds the generated-only enqueue
   marker and runs `agent-doc queue sync <target>`; ordinary backlog items are not
   activated. A timestamp-bounded self-target write guard plus the stable id
   prevents the watcher's own filing write from feeding itself.

All log classification and every document mutation go through the binary. The
script contains no document parsing or writing — see
`src/agent-doc/AGENTS.md` ("All deterministic behavior in the binary").

## Routing invariants

For source document `d`, cluster `c`, and resolved policy `P(d)`:

- `file(c) = d ∧ P(d).enabled` is required before any filing effect exists.
- `destination(d) = P(d).target`; a CLI `--target` may replace the destination
  only after `P(d).enabled` is proven and therefore cannot turn dogfood on.
- `auto(c, t) = ∃d. contributes(d, c, t) ∧ P(d).auto`; otherwise the only
  allowed effect is backlog filing.
- `Unknown` worker liveness is diagnostic evidence, never permission to project
  a running worker or mutate claim lifecycle.
- A self-target event is suppressed only when its typed timestamp falls inside
  this watcher's recorded binary-mutation interval. Stable ids independently
  make every filing idempotent across restarts.

## Usage

```bash
bin/agent-doc-dogfood-watch --once --dry-run          # see what would be filed
bin/agent-doc-dogfood-watch --once                    # file high/critical now
bin/agent-doc-dogfood-watch --interval 600            # poll every 10 minutes
bin/agent-doc-dogfood-watch --once --min-severity medium \
  --exclude-family 'session-review guardrail'         # widen, minus known noise
```

Key flags: `--target` (explicit destination override for already opted-in
sessions; it does not enable dogfood), `--min-severity` (default `high`),
`--exclude-family`, `--limit` (trailing log lines per project, default 4000).

## Why the default is `--min-severity high`

At `medium` the classifier includes routine guardrail traffic — `commit_noop
reason=already_current` is the loudest — which is normal closeout behavior, not a
defect. Filing it would bury real findings. Run `medium` deliberately, with
`--exclude-family` for families you have already judged to be noise.

## Idempotence

Three independent guards, so a re-run never duplicates:

- `known_ids(target)` reads `agent:backlog`, `agent:review`, `agent:icebox`,
  `agent:done`, and `agent:queue` — a finding that was filed and **closed** is
  not re-filed while the same line is still in the log tail.
- `.agent-doc/dogfood-watch-state.json` records what was filed, keyed by target
  document, so filing into a scratch target cannot suppress the same finding in
  the real bug document.
- For a current-document target, the ledger records the exact timestamp window
  of the binary-owned backlog/queue mutation and ignores only events emitted in
  that window. Later independent failures in the same document remain visible.
