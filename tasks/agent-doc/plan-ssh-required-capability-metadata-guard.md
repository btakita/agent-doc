# Plan — fail closed when SSH-dependent task docs omit required SSH metadata

## Problem

`agent-doc` already has a required-SSH capability contract, but it only engages
when the document explicitly declares `required_ssh_targets`.

The current `monsterrodholders` repro shows the gap:

- the site runbook at `src/boost-client/src/monsterrodholders-dev/AGENTS.md`
  clearly documents production SSH access through `monsterrodholders-server`
  / `root@50.28.2.199`
- the task asked for a production-side action: publish hidden test product
  `86558`
- the live shell environment returned `ssh root@50.28.2.199` ->
  `socket: Operation not permitted`
- `tasks/monsterrodholders.md` did not declare `required_ssh_targets`, so the
  existing fail-closed capability path never activated
- the response fell back to "here is the command / here is the URL" guidance
  instead of surfacing that the task was blocked on missing SSH capability

This is not the same bug as bare-socket SSH EPERM drift detection. That
detector already exists for documents that explicitly require SSH. The gap here
is missing or forgotten capability metadata on SSH-dependent task docs.

## Goal

Make SSH-dependent ops documents hard to run without an explicit required-SSH
contract, so `agent-doc` either proves the capability up front or fails closed
with a blocker instead of letting the task continue as if production access
might still be available.

## Proposed Fix

### 1. Add a durable way to declare SSH requirements for ops docs

Do not rely on free-text prompt inference alone. Add one deterministic path:

- document frontmatter remains the source of truth when
  `required_ssh_targets` is already present
- add a repo-local mapping surface for known ops docs or doc globs, for example:
  - task path `src/boost-client/tasks/monsterrodholders.md`
  - or a site profile name that expands to
    `required_ssh_targets: [monsterrodholders-server]`
- allow explicit frontmatter to override or extend the mapped default

This keeps the contract reviewable and avoids heuristic guessing from arbitrary
user prose.

### 2. Fail preflight when an SSH-dependent doc has no resolved requirement

Teach `agent-doc preflight` / start-up validation to resolve the effective
required SSH targets before launching the agent.

If the document is known to be an SSH-dependent ops doc but no target resolves:

- stop before launch
- print the concrete fix, such as adding
  `required_ssh_targets: [monsterrodholders-server]`
  or the relevant profile entry
- do not silently continue with a best-effort session

### 3. Reuse the existing required-SSH capability probe at runtime

Once the document resolves to a required SSH target, do not invent a second SSH
path. Route it through the existing capability contract:

- prove SSH capability before trusting resume / reroute
- treat `socket: Operation not permitted` and related SSH failures as capability
  loss
- retry fresh once when that contract already says to do so
- fail closed if the fresh launch still cannot prove the SSH requirement

The new work here is the metadata-resolution guard, not a new runtime SSH
detector.

### 4. Tighten final-response behavior for blocked required SSH

When the task depends on resolved required SSH and the probe fails, the final
answer should say the task is blocked on unavailable SSH capability.

It may still include the exact command or URL as operator context, but it must
not frame the action as completed, nearly completed, or implicitly available
from the current session.

### 5. Add regression coverage

Cover at least:

- a known SSH-dependent doc path resolves
  `required_ssh_targets: [monsterrodholders-server]` even without explicit
  frontmatter
- preflight fails closed when the doc/profile should require SSH but no target
  mapping can be resolved
- a resolved SSH-dependent doc with blocked `ssh ... -> socket: Operation not
  permitted` enters the existing required-SSH blocker path rather than ordinary
  task fallback
- explicit document frontmatter still overrides the mapped default when needed

## Exit Criteria

- `monsterrodholders`-style ops task docs cannot accidentally bypass the
  required-SSH contract just because frontmatter omitted the target metadata
- blocked SSH is surfaced as a hard capability blocker before or during the
  task, not as an ordinary shell failure
- user-facing output clearly says whether the task is blocked versus merely
  providing manual follow-up commands
