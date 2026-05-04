# Command Synonyms

Use this runbook when the user asks for `agent-doc` task orchestration in natural language instead of explicit CLI flags.

## Goal

Map orchestration phrasing to `agent-doc orchestrate <FILE> --mode ...` instead of manually simulating the batch.

## Dispatch Rule

- Prefer `agent-doc orchestrate` when the user is asking to coordinate multiple tasks or steps for the same session document.
- Preserve the user's task text. Only normalize the orchestration mode.
- If the user already named `orchestrate`, `parallel`, `dag`, or an explicit `--mode`, that explicit form wins.
- If the phrasing only describes one ordinary task instead of a batch, do not force orchestration.

## Mode Synonyms

| Mode | Dispatch | Natural-language triggers |
|------|----------|---------------------------|
| `sequential` | `agent-doc orchestrate <FILE> --mode sequential` | `orchestra`, `orchestrate`, `chain`, `in order`, `one by one`, `sequentially`, `synchronous`, `run these sequentially` |
| `parallel` | `agent-doc orchestrate <FILE> --mode parallel` | `fan out`, `concurrent`, `at the same time`, `in parallel`, `simultaneously`, `parallelize these` |
| `dag` | `agent-doc orchestrate <FILE> --mode dag` | `dependency graph`, `depends on`, `after X do Y`, `fan in`, `prerequisite`, `then unblock` |

## Tie-Breakers

1. If the prompt names explicit dependencies (`after`, `depends on`, `blocked by`), choose `--mode dag`.
2. Otherwise, if the prompt asks for concurrency (`fan out`, `concurrent`, `simultaneously`), choose `--mode parallel`.
3. Otherwise, if the prompt asks for ordered batching (`chain`, `in order`, `one by one`, `orchestrate`), choose `--mode sequential`.
4. If the wording is ambiguous but still clearly asks for orchestration, default to `--mode sequential`.

## Examples

- `Run these in order` → `agent-doc orchestrate <FILE> --mode sequential`
- `Chain these prompts without reusing context` → `agent-doc orchestrate <FILE> --mode sequential`
- `Fan out these benchmark tasks` → `agent-doc orchestrate <FILE> --mode parallel`
- `Do #bench after #prep, then summarize both` → `agent-doc orchestrate <FILE> --mode dag`
