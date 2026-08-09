# Dogfood Watch — live agent-doc sessions → backlog + queue

`bin/agent-doc-dogfood-watch` turns issues in running agent-doc sessions into
tracked work automatically. It is the dogfooding feedback loop for this repo:
several long-lived supervisors run against different documents, and defects that
show up in their logs used to be found only when a human happened to read
`ops.log`.

## What it does

1. Enumerates every live `agent-doc start` supervisor (`ps` + `/proc/<pid>/cwd`,
   so relative document paths under submodules resolve correctly) and keeps the
   tracked set: `agent-doc-bugs2`, `lazily`, `haiven-dev`, `sdk`,
   `monsterrodholders`. `--all-sessions` widens it; `--doc PATH` adds one
   explicitly.
2. Resolves each document's project root (nearest `.agent-doc/`) and asks the
   binary to classify that project's log: `agent-doc ops summary --json
   --project-root <root>`. Supervisor panics/aborts/OOM in
   `.agent-doc/logs/supervisor-stderr.log` are matched separately, since
   `ops summary` does not cover that file.
3. Aggregates clusters by **defect signature** — family plus an example line with
   session/cycle/pid/hash noise stripped — so one defect hitting four projects
   files one item naming all four, not four duplicates.
4. Files each new signature with `agent-doc backlog <target> add "id=dfw<hash> ..."`
   and mirrors it into the go queue with `agent-doc queue sync <target>`.

All log classification and every document mutation go through the binary. The
script contains no document parsing or writing — see
`src/agent-doc/AGENTS.md` ("All deterministic behavior in the binary").

## Usage

```bash
bin/agent-doc-dogfood-watch --once --dry-run          # see what would be filed
bin/agent-doc-dogfood-watch --once                    # file high/critical now
bin/agent-doc-dogfood-watch --interval 600            # poll every 10 minutes
bin/agent-doc-dogfood-watch --once --min-severity medium \
  --exclude-family 'session-review guardrail'         # widen, minus known noise
```

Key flags: `--target` (document that receives the items; defaults to
`tasks/agent-doc/agent-doc-bugs2.md`), `--min-severity` (default `high`),
`--exclude-family`, `--limit` (trailing log lines per project, default 4000).

## Why the default is `--min-severity high`

At `medium` the classifier includes routine guardrail traffic — `commit_noop
reason=already_current` is the loudest — which is normal closeout behavior, not a
defect. Filing it would bury real findings. Run `medium` deliberately, with
`--exclude-family` for families you have already judged to be noise.

## Idempotence

Two independent guards, so a re-run never duplicates:

- `known_ids(target)` reads `agent:backlog`, `agent:review`, `agent:icebox`,
  `agent:done`, and `agent:queue` — a finding that was filed and **closed** is
  not re-filed while the same line is still in the log tail.
- `.agent-doc/dogfood-watch-state.json` records what was filed, keyed by target
  document, so filing into a scratch target cannot suppress the same finding in
  the real bug document.
