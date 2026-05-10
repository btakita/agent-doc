# Plan — fix Codex sandbox network denial in agent-doc sessions

**Pending:** `#codexnonet`
**Status:** Implemented

## Problem

Codex sessions launched or reused through `agent-doc` can run with an effective
sandbox that blocks all network calls, even when the intended session contract
requires network access and the visible launch args look permissive, such as
`-s danger-full-access`.

The user-facing symptom is not just one SSH or DNS edge case. The report is that
the Codex sandbox is not allowing any network calls. That means agent-doc may be
launching, resuming, clearing, or reusing Codex panes without proving the live
network policy that the document/session needs.

Related work:

- `#codexcapstale` covers managed-session capability proof before pane reuse.
- `plan-codex-clear-permissions.md` covers capability loss after `/clear`.
- Required-SSH plans cover narrower SSH drift after resume.

This item should define the broader invariant: if a document or preset requires
network access, agent-doc must not dispatch work into a Codex runtime whose live
sandbox denies outbound network.

## Desired Behavior

1. Documents that require network access declare that requirement explicitly, or
   agent-doc derives it from existing fields such as `required_ssh_targets`.
2. Before dispatching into a Codex pane for network-dependent work, agent-doc
   proves the live child can make at least one cheap outbound network request or
   hostname resolution appropriate to the requirement.
3. If the proof fails, agent-doc does not let the model guess at application
   causes. It classifies the failure as a Codex sandbox/network capability
   failure.
4. Recovery is deterministic:
   - restart/rebind the managed Codex pane once with the full launch contract
   - re-run the network proof
   - proceed only after success
   - otherwise fail closed with exact remediation guidance
5. Diagnostics distinguish:
   - host network is down
   - Codex sandbox network is disabled
   - DNS resolution failed
   - SSH alias/config failed
   - remote service refused the connection

## Investigation Steps

- [ ] Capture a minimal repro from an agent-doc managed Codex pane:
  - `getent hosts github.com` or equivalent DNS probe
  - `curl -I https://github.com` or another cheap HTTPS probe when available
  - one user-relevant network command from the failing workflow
- [ ] Run the same probes from the outer host shell or a fresh Codex runtime to
      separate host/network failure from managed-pane sandbox failure.
- [ ] Trace the Codex launch path that decides sandbox/network policy:
  - frontmatter `codex_args`
  - installed Codex instructions/config
  - agent-doc `start`, route, resume, clear, and controller reuse paths
  - any environment variables such as `CODEX_SANDBOX_NETWORK_DISABLED`
- [ ] Decide whether network requirements should be expressed as:
  - a new frontmatter field, for example `codex_network_access: enabled`
  - derived capability requirements from SSH/URL/tool usage
  - both, with the explicit field taking precedence
- [ ] Audit current network-related failures and merge or cross-link duplicates
      once the invariant is clear.

## Implementation Steps

- [ ] Add a Codex network capability probe layer that can run before dispatch
      and after lifecycle transitions.
- [ ] Persist the last successful proof with the actor generation and launch
      contract so stale proofs cannot survive `/clear`, resume, restart, or
      controller handoff.
- [ ] Restart the managed pane once on failed proof when a fresh launch contract
      should provide network access.
- [ ] Fail closed after retry with a diagnostic that names the failed probe and
      whether the host/fresh runtime could reach the network.
- [ ] Update runbooks/specs so network-capable Codex sessions are documented as
      a proven live capability, not inferred from process args alone.

## Regression Coverage

- [ ] A managed Codex pane with `-s danger-full-access` but blocked network is
      not reused for a network-required document.
- [ ] A blocked reused pane is restarted once and re-probed before dispatch.
- [ ] Fresh retry success clears the network-denial marker.
- [ ] Fresh retry failure returns a precise sandbox/network diagnostic.
- [ ] Host network failure is not mislabeled as Codex sandbox denial.
- [ ] DNS-only failure and HTTPS/connect failure produce distinct diagnostics.

## Exit Criteria

- `agent-doc session status` can report whether the current Codex actor has
  proven live network access for the active document.
- Network-required workflows no longer proceed inside a Codex pane whose sandbox
  blocks outbound network.
- The user receives actionable diagnostics instead of application-level guesses
  when the real blocker is the Codex sandbox network policy.
