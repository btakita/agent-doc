# Multi-user workspace access control (`#ghrecvpolicy`)

Status: phases 1 and 2 landed (identity and membership model,
controller-socket admission, per-request authorization, session ownership,
and the recipient `[receive]` policy on every component-creation path, which
closes GH #40). Phases 3 through 6 are open.

Tracking: [GH #40](https://github.com/btakita/agent-doc/issues/40) (recipient
policy), backlog `[#ghrecvpolicy]`.

## Why

agent-doc assumes one operator. [Security](../10-security.md) says so, and
several features already expose the gap:

- `notify --backlog-add` creates `agent:backlog` on any target document the
  caller can write. The sender, not the document's owner, decides what appears
  there (GH #40).
- Focused `extract` adds components to an existing target document.
- `queue add --source` lets external tools append runnable work.
- The project controller serves every document and pane in a project and
  admits any process running as the controller's user.

Before more than one person shares a workspace, agent-doc needs a principal
model, a membership model, and an enforcement point on every channel. The
single-user default must keep today's behaviour exactly.

## Threat model

**In scope.**
- Several OS accounts on one machine share a project directory, for example
  through a common group and group-writable files.
- A member acts beyond their role: an editor tries to drive another member's
  agent pane, or creates components on a document whose owner did not allow it.
- A member's agent is prompt-injected and uses agent-doc commands to reach
  another member's sessions or documents.
- A process of an unrelated local user connects to the project controller.

**Out of scope.**
- root, the kernel, and anyone who can ptrace a member's processes.
- A member writing another member's files directly with OS write permission.
  For direct file I/O, OS file permissions are the only hard boundary. agent-doc
  policy governs operations that go through agent-doc: the controller, its
  commands, and its editor replicas.
- Network clients of `agent-doc serve`. Bearer tokens already cover them;
  phase 6 maps those tokens to principals.

**Trust assumptions.** The kernel's peer credentials (`SO_PEERCRED` on Linux,
`getpeereid`/`LOCAL_PEERCRED` on macOS) identify a Unix-socket peer. A file's
owner uid identifies its owner. Document content is untrusted (unchanged from
today).

## Principals

Every channel resolves to one **principal**: the OS user (effective uid)
behind the process or connection.

| Actor | Principal | How it is established |
|---|---|---|
| CLI process (`notify`, `extract`, `write`, `queue add`) | its effective uid | `geteuid()` |
| Agent / harness session (Claude, Codex, ...) | the uid running the harness | inherited by every agent-doc command it runs |
| Supervisor | the uid that started it | same as the session |
| Editor client (JetBrains, VS Code, Zed) | the editor process's uid | controller-socket peer credentials |
| `agent-doc serve` browser client | phase 6: the principal its token maps to | bearer token |
| External tool via `queue add --source` | the uid running the tool | `geteuid()`; `--source` is a provenance label, not an identity |

A session or pane is not a principal. It is a resource **owned** by the
principal that started it (phase 2 records that owner).

## Workspace membership and roles

The **workspace** is the project store (the directory holding `.agent-doc/`).
Its **owner** is the OS user that owns `.agent-doc/`, which is also the user
the project controller runs as (the controller already requires an
owner-owned socket directory).

`.agent-doc/config.toml`:

```toml
[workspace]
mode = "multi-user"          # default "single-user"

[[workspace.members]]
uid = 1001
name = "alice"               # display only
role = "editor"              # owner | editor | viewer

[receive]
auto_create_components = ["backlog"]   # true (default) | false | [] | [names]
```

| Role | Capabilities |
|---|---|
| `owner` | Everything: controller administration, every session and pane, every document. The workspace owner always holds it; members may be co-owners. |
| `editor` | Read and write documents it may access, drive sessions it owns, send into other members' documents subject to their `[receive]` policy. |
| `viewer` | Read documents and status. No writes, no session control. |

Rules (pure, `agent_doc_frontmatter::workspace_access::role_of`):

- The owner always has `owner`.
- In `single-user` mode no one else has a role. Member entries are ignored, so
  listing members never widens access until the mode says so.
- In `multi-user` mode a listed member has its entry's role, and anyone else
  has none.

## Identity per channel

1. **Controller socket (phase 1, landed).** The controller reads the peer's
   kernel credentials before it reads a request. `controller_peer_decision`
   admits every principal with a role (the controller's user as `owner`, and
   in multi-user mode each listed member with its role) and refuses everyone
   else with a reason:
   - not the owner (single-user);
   - not a member (multi-user).

   The owner is admitted without file I/O. Other peers read `[workspace]`
   through an mtime/length-keyed cache, so membership edits apply without a
   restart. An unreadable or unparseable config falls back to single-user.
   Today the socket directory is `0700` and the socket `0600`, so in practice
   only the owner can connect at all. Phase 3 opens the transport.
2. **Controller RPC (phase 2, landed).** The admitted principal and role
   travel with the request in a thread-scoped context beside the existing
   peer pid (`CONTROLLER_REQUEST_PEER_PID`); see
   `agent-doc-controller-io/src/project_controller/request_authz.rs`. Each
   command is classified by the capability it needs
   (`CONTROLLER_COMMAND_CAPABILITIES`); non-owners are denied anything
   unclassified. A principal other than the controller's user is also the
   sender that component creation on that request thread is attributed to.
3. **Editor replicas (phase 4).** A replica's registration records the
   principal from the socket that registered it. A `viewer` replica is
   read-only, and CRDT ops are attributed to their principal in the state
   ledger.
4. **CLI on files (phase 1 for component creation).** Commands that change
   another workspace's document compare the sender (`geteuid()`) with the
   recipient workspace (owner of the target's `.agent-doc/`, plus its config).
5. **`queue add --source` (phase 6).** The process principal is the sender.
   A non-owner sender's head lands as a review item rather than runnable
   queue work, unless the recipient's policy admits that sender.
6. **`agent-doc serve` (phase 6).** Each token maps to a principal and role.
   A read-only token is a `viewer`.

## Per-document and per-component permissions (phase 5)

The default is the workspace role. A document may narrow it:

- `agent_doc_owner: <uid|name>` (default: the file's owner uid) names the
  principal whose `[receive]` decisions and session the document follows.
- `agent_doc_collaboration: shared` plus `agent_doc_security_review` (existing)
  stays the gate for cross-document reads. Phase 5 replaces "is shared" with
  "is the reader a member allowed to read both documents".
- `agent_doc_members` (optional) restricts writers to a subset of workspace
  members.

Component-level rules start with creation (`[receive]`, below). Later rules
may restrict who can edit `agent:queue` (runnable work) separately from
`agent:exchange`.

## Session isolation (phases 2 and 4)

A session (its supervisor, agent pane, and claim) belongs to the principal
that started it. Phase 2 records it in the session-owner ledger
(`.agent-doc/session-owners.toml`, keyed by registry key) at claim/start/attach,
only for sessions a non-owner takes and only once the request succeeds. An
absent key is the workspace owner's session, so a single-user project never
writes the ledger. (The tmux-router registry entry itself is unchanged.)

- **Drive.** `route`, `queue` dispatch, `start`/`restart`, send-keys,
  focus layout, and lifecycle require `owner` role or session ownership.
  Cross-session layout sync and `session` admin stay owner-only. A
  drive request must name a document, or a pane bound to exactly one; an
  owner taking a session drops the earlier member record.
- **Claim.** `agent-doc claim` by a member asks the running controller
  (`session_owner_claim`) before it mutates a file; the controller reads the
  principal from the socket credentials and refuses a session another
  principal holds. With no controller running the claim fails closed.
- **Observe.** Status, read, and dashboards need any role. A future option
  could hide other members' sessions.
- **Agent pane input.** Only the session owner's principal can cause input to
  reach its pane. A prompt-injected agent of another member is refused at the
  controller, not at tmux.
- **tmux.** Each member runs their own tmux server under their own uid. The
  controller never sends keys to a pane whose server belongs to another uid.
  The live-tmux guard (`#tmuxkittyterminal`) stays as is.

## Recipient-side `[receive] auto_create_components` (GH #40, phase 1, landed)

The recipient decides what senders may create on its documents.

| Value | Effect for a non-owner sender |
|---|---|
| unset or `true` | any missing component may be created (today's behaviour) |
| `false` or `[]` | refused with an error naming the component, the policy, and the owner to ask |
| `["backlog", ...]` | only the listed components (`pending` is `backlog`; `run`/`loop` are `queue`) |

- The recipient's owners are never limited by their own policy.
- A sender-side opt-out (`--no-create-backlog`) still applies first, whatever
  the recipient allows.
- A component that already exists needs no permission.
- In single-user mode every sender who can reach the document is its owner, so
  the policy has no effect. That keeps today's behaviour.

Enforced in:

- `notify --backlog-add` / `--backlog-add-gated` (`src/notify.rs`, before
  auto-creating `agent:backlog`).
- Focused `extract` into an existing target (`src/focused_extract.rs`): every
  component the plan adds to the target is checked before the preview prints.
  A new target document is document creation, which phase 5 governs.

Already strict, so no change: `write --backlog-add-to` refuses a target with
no backlog. `transfer` appends to the end of a target that lacks the component
rather than creating it.

Phase 2 covers the rest. The sender is the process's user, or inside the
controller the request's principal. The binary's write boundaries compare the
document before and after each write (`created_component_names`, a marker
scan confirmed by the component parser) and check every new component:

- route dispatch creating `agent:run` (`insert_queue_component`);
- tracked-work command writes (`agent:review`, `agent:done`, `agent:icebox`,
  ...);
- response writes through template patches (`patch:todo` scaffolding
  `agent:todo`, the recap section, any component the agent adds);
- prompt-blocker hoists creating `agent:review`;
- editor convergence (`converge_document_or_disk`, `converge_or_disk_write`)
  and rebased disk writes;
- `claim`'s empty-file and default-component scaffolds.

A write that creates no component does no extra I/O.

Pure rule: `component_auto_create_decision`. Adapters:
`agent_doc_project_config_io::workspace_access::enforce_component_auto_create`
and `enforce_created_components` / `enforce_sender_created_components`.

## Migration

- No config means `single-user` and a permissive `[receive]`. Every existing
  project behaves exactly as before, and the controller admits exactly the
  same peers (its own uid).
- A project opts in by setting `[workspace] mode = "multi-user"` and listing
  members. Until phase 3 the socket stays owner-only, so opting in changes
  nothing at runtime except members' admission and per-request authorization
  (once they can reach the socket) and the `[receive]` policy for non-owner
  senders.
- `[receive]` can be set in single-user mode too. It then governs cross-user
  senders on the machine (another account that can write the file), and has
  no effect for the owner.

## Phases

Each phase lands with its tests and leaves single-user behaviour unchanged.

1. **Identity and membership; controller admission; GH #40 (landed).**
   - `agent_doc_frontmatter::workspace_access`: principal, roles, `[workspace]`,
     `[receive]`, pure decisions, and AccessWorld scenario tests with distinct
     principals.
   - `agent_doc_project_config_io::workspace_access`: current principal,
     recipient workspace resolution, enforcement.
   - Controller admission through `ControllerWorkspaceAccess`.
   - `[receive]` enforced in `notify` and focused `extract`, tested end to end
     with distinct sender and recipient uids.
2. **Per-request authorization and session ownership (landed).**
   - Admitted principal and role in the request context.
   - Command capability table (observe, write-document, claim-session,
     drive-session, administer); deny-by-default for non-owners.
   - Session owner recorded on claim/start/attach in the session-owner
     ledger; drive operations check it.
   - `editor`/`viewer` admission at the socket, enabled with this phase.
   - The remaining component-creation paths, in and outside the controller.
3. **Shared controller transport.**
   - `[workspace] group = "<name>"`: a group-owned socket directory (`0750`
     or `0770`) and socket (`0660`), with startup validation that the group
     matches.
   - Never enabled without phase 2.
4. **Editor replica attribution.**
   - The registering principal stored on each replica.
   - `viewer` replicas cannot write.
   - CRDT ops attributed in the state ledger.
   - Per-member tmux server ownership checks.
5. **Per-document ACL.**
   - `agent_doc_owner`, `agent_doc_members`.
   - Membership-based cross-document read gates replacing the shared/review
     heuristic.
   - Document creation in another member's workspace.
6. **Inbound channels.**
   - `serve` tokens map to principals.
   - `queue add --source` from a non-owner sender lands as review unless
     admitted.
   - Content scanning for inbound text (the existing planned item).

## Tests

- Pure: `agent-doc-frontmatter/src/workspace_access.rs`. These cover role
  resolution, controller decisions, TOML forms, and AccessWorld scenarios. The
  scenarios use two workspaces with distinct owners and several principals
  creating components in order.
- Adapter: `agent-doc-project-config-io/src/workspace_access.rs`. These use a
  real store, real config, and a distinct sender uid.
- Controller: `agent-doc-controller-io/src/project_controller/workspace_access.rs`.
  These cover the single-user default, membership reload without restart, and
  the fallback to single-user on a bad config.
- Controller authorization (phase 2):
  `agent-doc-controller-io/src/project_controller/request_authz.rs`. Distinct
  simulated principals (owner, two editors, a viewer, a stranger) talk to a
  real controller runtime over a real Unix socket through `serve_client`;
  a test-only override supplies the peer's effective uid, since the socket is
  still `0700`/`0600`. They cover role limits, one editor never driving
  another's session, the owner retaking a session, no record for a failed
  claim, and the single-user default reading no workspace files (zero config
  reads, one ledger existence check per controller, no ledger created). A
  source test keeps the capability table in step with the dispatcher.
- Pure session world: `SessionWorld` in
  `agent-doc-frontmatter/src/workspace_access.rs`.
- Component-creation paths: `agent-doc-element/src/created_components.rs`,
  `agent-doc-route-io/tests/route.rs`
  (`route_enqueue_dispatch_prompt_follows_recipient_receive_policy`),
  `agent-doc-element-backlog-io` (`review_scaffold_follows_recipient_receive_policy`),
  and `agent-doc-template-io` (`response_component_scaffold_follows_recipient_receive_policy`).
- End to end: `src/notify.rs` `recipient_policy_*` (real documents written or
  left untouched, via `run_as` with a distinct sender) and
  `src/focused_extract.rs` recipient-policy tests.
