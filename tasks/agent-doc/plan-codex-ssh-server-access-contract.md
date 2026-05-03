# Plan: prove and preserve Codex SSH access for required remote hosts

## Problem

`agent-doc` can launch Codex with `codex_args: -s danger-full-access` and `codex_network_access: enabled`, yet a routed or resumed Codex session can still lose the ability to reach the remote SSH target that the document depends on.

For the current `monsterrodholders` workflow, the practical requirement is access to the production server behind the `monsterrodholders-server` SSH target (`root@50.28.2.199`). Recent failures were not one clean class:

- some turns could not open outbound SSH sockets to `50.28.2.199`
- some turns could not resolve `github.com`, implying broader DNS/network degradation
- some git/SSH flows were sensitive to SSH-config readability, while direct `ssh -F /dev/null -i ~/.ssh/id_ed25519 ...` could get further
- fresh outer Codex sessions sometimes had working network/SSH while resumed `agent-doc` Codex sessions did not

So today `agent-doc` can request a capability envelope, but it cannot prove that the live Codex child still has the specific remote-access capability the task requires.

## Goal

Make remote SSH access a first-class, probeable launch contract for Codex-backed `agent-doc` sessions:

1. a document can declare that certain SSH targets are required
2. `agent-doc` proves that capability before trusting a resumed/routed session
3. capability drift triggers deterministic recovery or a fail-closed blocker
4. diagnostics distinguish alias/config issues from general network loss and from stale resumed-session capability drift

## Proposed Fix

### 1. Add first-class required-remote-access metadata

Add an explicit document/config surface for SSH-dependent sessions, for example:

- frontmatter like `required_ssh_targets: [monsterrodholders-server]`
- or a more general `required_capabilities` / `capability_probes` surface that can express:
  - SSH alias target: `monsterrodholders-server`
  - direct fallback target: `root@50.28.2.199`
  - optional probe variants such as `direct-no-config`

For the `monsterrodholders` class of docs, the contract should let agent-doc distinguish:
- "the session cannot open outbound SSH at all"
- "the session can SSH directly, but cannot read the alias/config path"
- "the session is fully capable and may proceed"

### 2. Probe SSH capability before trusting resume/reuse

Before `agent-doc` routes work that depends on production SSH, probe the live Codex child instead of assuming that:
- `codex_args: -s danger-full-access`
- `codex_network_access: enabled`
- and an old `resume:` id

are enough.

Probe strategy:
- alias probe: `ssh -o BatchMode=yes -o ConnectTimeout=5 monsterrodholders-server true`
- direct probe fallback: `ssh -F /dev/null -i ~/.ssh/id_ed25519 -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=5 root@50.28.2.199 true`

This keeps `agent-doc` from conflating three different states:
- network/socket denied
- DNS resolution failure
- SSH-config / alias readability failure

### 3. Treat SSH probe failure as capability drift, not just task failure

If a previously healthy Codex document declares required SSH access and the probe now fails with signatures such as:
- `socket: Operation not permitted`
- `ssh: connect to host ... port 22: failure`
- `Could not resolve hostname ...`
- SSH-config include/readability errors

then `agent-doc` should treat that as a capability-contract failure on the session, not merely as one failed shell command.

Recovery order:
- if the session is resumed/rerouted, discard or bypass the stale `resume:` thread and retry once with a fresh `codex exec`
- reapply the resolved sandbox/network policy on that fresh launch
- rerun the SSH probe
- only proceed with the task if the fresh launch proves the required SSH capability

If fresh launch still cannot prove it, fail closed with a blocker instead of pretending the session is production-capable.

### 4. Preserve successful capability proof across the session lifecycle

Track the last successful probe result as part of the session/runtime metadata so `agent-doc` knows:
- this document previously had proven SSH access
- a later failure is a regression/capability drift event
- `/clear`, fresh reroute, or resume replacement should invalidate that proof and require re-probe

This should integrate with the existing Codex capability-drift handling rather than living as an unrelated special case.

### 5. Improve user-facing diagnostics

When a session cannot satisfy the required SSH contract, report the concrete class of failure:

- outbound SSH/network blocked
- DNS resolution unavailable
- SSH alias/config unreadable, but direct host path may still work
- resumed-session capability drift requiring fresh Codex exec
- fresh launch still lacks the required capability

The message should say whether the task is blocked, whether a fresh retry already happened, and whether the operator should use the alias path or the direct `ssh -F /dev/null ... root@50.28.2.199` path for investigation.

### 6. Add regression coverage

Cover at least:

- required SSH target metadata resolves from frontmatter/config
- a healthy resumed session with passing SSH probe is reused
- failed SSH probe on a resumed session forces one fresh Codex exec retry
- alias failure plus direct success is diagnosed as config/alias degradation, not full network loss
- direct-host failure with DNS/socket signatures is diagnosed as network/SSH capability failure
- `/clear` and resume-replacement paths invalidate old SSH capability proof and require re-probe

## Exit Criteria

- A Codex-backed `agent-doc` session that requires `monsterrodholders-server` access does not proceed on blind trust of `resume:` alone.
- `agent-doc` can prove whether the live session has production SSH capability before routing production work.
- Resumed-session capability drift triggers one deterministic fresh retry instead of repeated broken reuse.
- Diagnostics clearly separate alias/config breakage from true remote-network loss.
- If required SSH capability cannot be proven, `agent-doc` fails closed instead of implying that production SSH is available.
