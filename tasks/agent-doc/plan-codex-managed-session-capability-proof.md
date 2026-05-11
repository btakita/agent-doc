# Plan — prove managed Codex session capabilities before reuse

**Pending:** `#codexcapstale`
**Status:** Draft

## Problem

Fresh outer Codex sessions can have working DNS and SSH while the long-lived
Codex session managed by `agent-doc` still reports sandbox-like failures such as:

- `Could not resolve hostname github.com`
- `socket: Operation not permitted`
- inability to write submodule git metadata that should be covered by launch
  writable roots

Recent fixes improved individual paths:

- launch/resume args preserve `sandbox_mode="danger-full-access"`
- `required_ssh_targets` can detect some SSH drift and retry fresh
- tracked `/clear` can force a fresh restart on selected managed route paths

Those fixes still do not create one general invariant: before reusing a ready
managed Codex pane for a document that needs network, SSH, or expanded writable
roots, `agent-doc` must prove the live child actually has those capabilities.

## Desired Behavior

1. A ready Codex actor is not trusted solely because its process args include
   `-s danger-full-access` or because the controller reports `ready`.
2. For documents declaring `codex_network_access: enabled`,
   `required_ssh_targets`, or submodule write roots, agent-doc runs cheap
   capability probes before dispatch/reuse when the previous turn showed
   capability drift or after lifecycle events such as `/clear`, resume, restart,
   controller handoff, or stale actor recovery.
3. If a fresh outer/current runtime can perform the operation but the managed
   pane cannot, the failure is classified as managed-session capability drift,
   not as a host-wide sandbox outage.
4. Recovery restarts the managed pane fresh once with the original launch
   contract, re-probes, then either proceeds or fails closed with the exact
   failing capability.

## Implementation Steps

- [ ] Add a capability-proof layer for Codex managed sessions:
  - DNS probe when `codex_network_access: enabled`
  - SSH probe for each `required_ssh_targets` entry
  - writable-root probe for each gitdir/submodule root added via `--add-dir`
- [ ] Persist probe results and invalidation triggers in session/controller state:
  - `/clear`
  - resume/restart
  - actor generation change
  - stale-controller replacement
  - any transcript with network/DNS/SSH/write-root sandbox signatures
- [ ] Run the proof before route/dispatch reuses an existing Codex pane when a
  document declares capabilities or recent drift evidence exists.
- [ ] On probe failure, restart that same managed session fresh once with the
  resolved launch contract, then re-run the probes before dispatch.
- [ ] Improve diagnostics to distinguish:
  - current host/runtime lacks the capability
  - managed Codex child lacks the capability
  - SSH alias/config failure
  - DNS failure
  - filesystem writable-root failure

## Regression Coverage

- [ ] Ready actor with correct process args but failing DNS is not reused blindly.
- [ ] Ready actor with correct process args but failing required SSH is restarted
  fresh once and re-probed.
- [ ] Fresh retry success clears the drift marker and allows dispatch.
- [ ] Fresh retry failure fails closed with a capability-specific diagnostic.
- [ ] A writable-root probe covers nested submodule gitdir paths added through
  `--add-dir`.

## Exit Criteria

- `agent-doc session status` can explain whether a managed Codex pane has proven
  network, SSH, and writable-root capabilities for the document.
- The Monster Rod Holders workflow no longer reports sandbox/DNS/SSH failures
  from a reused pane while a fresh Codex runtime on the same host has those
  capabilities.
- Correct launch args remain necessary, but no longer substitute for live proof.
