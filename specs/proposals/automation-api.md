# Proposal: A Language-Agnostic Automation API

> **Status:** proposal, non-normative. Nothing here is implemented. Normative
> behavior stays in [`SPEC.md`](../../SPEC.md) and the numbered `specs/` files;
> a phase below becomes normative only when its queue item lands spec + tests.
> Citations are `path:line` against `origin/main` at `7a92742ece`.

## Question

> agent-doc should have an API for automation. This API should work across all
> languages. Should we make the controller an api server?

## Short answer

**The controller should be the authority behind the API, but its internal RPC
should not be the API.** Define a small, separately versioned public operation
set (`agent-doc/api/v1`), with one JSON request and response schema. Ship it in
three steps, all served by the same in-controller handlers:

1. a CLI entrypoint, `agent-doc api <op> --json`. Any language that can spawn a
   process can call it, and that is every language;
2. the same request and response bytes on the existing controller socket, as one
   new, version-negotiated controller command;
3. later, an HTTP/SSE facade inside the existing `agent-doc serve`, and MCP tools
   generated from the same operation table.

Do not build a separate long-lived daemon, gRPC, or a "bless the internal RPC"
API. The reasons are in [Should the controller be the API server?](#should-the-controller-be-the-api-server).

## What exists today

### The project controller already serves RPC, to three languages

- **Transport.** A local socket (`interprocess::local_socket`) at the fixed path
  `<root>/.agent-doc/controller.sock` (`agent-doc-controller/src/paths.rs:5`,
  `:15`). The path is bounded by `sun_path`, so deep project roots cannot
  bind it at all (`agent-doc-controller/src/paths.rs:19-50`).
- **Framing.** Each connection carries one newline-terminated JSON request and
  gets one newline-terminated JSON response. An idle client that sends no
  complete line is closed (`agent-doc-controller-io/src/project_controller/rpc.rs:21659-21707`).
  The one long-lived exception is `document_turn_authority_stream`, which is
  answered with a stream of lines on the same connection (`rpc.rs:21676-21682`,
  `rpc.rs:22591`).
- **Request shape.** One flat struct with `command`, `file`, `session_id`,
  `pane_id`, `window_id`, `generation`, `state`, `caller`, `reason`,
  `supervisor_pid`, `supervisor_socket`, `command_kind`, `diagnostic_payload`,
  and `sequence` (`agent-doc-controller-io/src/project_controller.rs:11004-11024`).
  The struct is `pub(crate)`. Typed payloads for newer commands are JSON
  *strings* inside `diagnostic_payload` (`rpc.rs:22597-22600`, `rpc.rs:31738-31740`),
  and some commands repurpose `state` or `caller` (for example, `coordination_claim`
  carries scopes in `state` and an owner token in `caller`, `rpc.rs:21844-21858`).
- **Response shape.** `{"ok":true,"data":…}` or `{"ok":false,"error":"<string>"}`
  (`rpc.rs:22720-22731`, client side `project_controller.rs:11057-11066`). A
  success always carries `data`, and JSON `null` means "absent", not "missing"
  (`specs/08b-single-process-control-plane.md:21-26`). Errors are prose strings
  with no stable code.
- **Size and churn.** The dispatch `match` in `handle_request_locked` has 106
  command names (`rpc.rs:21843-22575`), and an unknown name answers
  `unknown controller command` (`rpc.rs:22571-22574`). There are 236
  `ControllerRequest { … }` literals, every one inside `agent-doc-controller-io`
  (182 in `rpc.rs`, 54 in `project_controller.rs`). `rpc.rs` is 66k lines and
  took 50 commits in the 30 days before this proposal. The command set is an
  implementation detail that is still moving fast.
- **Versioning is lockstep, not semantic.** Clients stamp `binary_version` (and
  `binary_identity`) onto the request (`rpc.rs:2085-2091`). The controller
  refuses write-bearing commands (`command_plane_submit`, `dispatch`,
  `crdt_text_adopt`, `commit_document`, …) from a client whose version differs
  from its own (`rpc.rs:21822-21833`, `rpc.rs:22695-22697`). An *absent*
  `binary_version` passes as "an older client" (`rpc.rs:21823-21825`). So
  today's protocol tells a foreign client either "you must be exactly this
  build" or nothing at all.
- **Trust model.** `specs/10-security.md:5` says agent-doc is single-user and
  local, with "no authentication, authorization, or multi-user access control".
  I found no `set_permissions` call on the socket or its directory, and no
  peer-credential (`SO_PEERCRED`) check, in the controller sources. Access
  control is whatever the filesystem gives `.agent-doc/`.
- **Lifecycle.** The controller is lazily launched by its first client and
  retires when idle: after 30 minutes on a current binary, or 60 seconds on a
  superseded one. An install hands it off to a successor
  (`specs/08b-single-process-control-plane.md:729-760`). `Idle` counts
  `active_clients` (`08b:758`), so a long-lived subscriber keeps it alive. On
  handoff, a stream ends once a successor is promoted, and the client must
  reconnect (`rpc.rs:22634-22640`, `#drainingpredecessor`).
- **Callers today.**
  - The CLI, in-process through the same crate.
  - JetBrains, in Kotlin over `UnixDomainSocketAddress`
    (`editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaForwarder.kt:1094-1097`,
    `:1020`, `:776`).
  - VS Code, in TypeScript over `net.createConnection`
    (`editors/vscode/src/extension.ts:619`, `:1142-1175`;
    `editors/vscode/src/crdtReplica.ts:482-487`).

  The controller socket is already a multi-language wire. Each language
  hand-rolls the same untyped JSON, without a schema.

### A versioned envelope already exists: `command-plane-v1`

`editors/command-payloads.md:1-31` binds agent-doc commands to the Lazily
`CommandSubmit` envelope:

- `namespace: "agent-doc"`;
- a per-command `payload_type` of `agent-doc.<name>.v<major>`;
- a `payload_hash`;
- an `authority_generation` guard;
- an `idempotency_key`;
- a terminal `CausalReceipt` as the only proof of completion.

An unknown `payload_type` is rejected with `unknown_payload_type`, never applied
(`editors/command-payloads.md:28-30`; enforcement in
`agent-doc-controller-io/src/project_controller/command_plane.rs:48-52`,
`:164-171`). Async submits survive a controller handoff under the same
`command_id` (`editors/command-payloads.md:42-48`). The envelope schema lives in
`lazily-spec` (`schemas/message-passing.json`), which already has bindings in
many languages.

This is the right *shape* for a public API: namespaced, per-operation major
versions, idempotency, generation fencing, and receipts. What it lacks is a
public operation set, stable error codes, and a client that doesn't need a
Lazily binding.

### The 08b Dispatch and Admin APIs

`specs/08b-single-process-control-plane.md:873-921` names `SubmitCommand`,
`QueryActor`, `QueryDispatch`, `QueueControl`, and `AdminControl`, with typed
results (`Rejected`, `Accepted`, `Queued`, `Running`, `Completed`, `Blocked`)
and durable `dispatch_attempts` receipts. `08b:923-955` names the admin
surface, with `admin_operation` as its receipt target. These are the intended
domain verbs. They are reached through CLI subcommands, not through a public
schema.

### The MCP server

`agent-doc mcp serve` is JSON-RPC over stdio, protocol `2025-06-18`
(`src/mcp.rs:7`, `:11-50`). It has six tools: `agent_doc_read`,
`agent_doc_admit`, `agent_doc_preflight`, `agent_doc_plan`,
`agent_doc_session_check`, and `agent_doc_finalize` (`src/mcp.rs:362-441`).
Every tool shells out to the agent-doc binary (`src/mcp.rs:566`, `:929`). A
mutating call re-execs a fresh binary when the server's binary is stale
(`src/mcp.rs:58-122`). The tool set is the *agent turn lifecycle*: admit, then
preflight, then finalize. It is not an automation surface. A CI job or
dashboard has no use for `finalize`, and MCP gives it no `queue add`, status, or
events.

### `agent-doc queue add`: the write path to copy

`specs/07-orchestration-commands.md:1048` (`#queueinbound`, GH #237) is the
documented way for "an external tool (an editor plugin, a game-engine
play-mode bridge, a CI watcher)" to hand work to a session. Every rule an
automation write needs is already in it:

- **Attribution.** The head is `[from:<source> key=<key>]`, with a restricted
  alphabet for the tag.
- **Idempotency.** A keyed head is deduped against live heads, and the document
  itself is the dedupe ledger.
- **Authority.** The head is planned against the *realtime current* document
  and written through `converge_document_or_disk`. While an editor holds the
  document, the change goes into the live CRDT replica, "never written to disk
  behind the buffer". Unsaved operator text survives.
- **Byte preservation.** No existing queue line is re-rendered.
- **Locking.** The queue-mutation lease is held across read, plan, and write.
- **Merge identity.** The head is keyed by its attribution tag, so a concurrent
  operator edit cannot drop it or splice into it.
- **Redaction and attachments.** The text is redacted, and an attachment becomes
  a content-addressed file instead of a multi-line queue entry.
- **Steering.** A running turn sees the head as `drain_after_current`.
- **Output.** `--json` prints `{"outcome":"added"|"duplicate",…}`.

### The CLI `--json` surface: a de facto API

There are 50 subcommands with a `--json` flag in `src/main.rs`. They include:

- `queue add` (`src/main.rs:4479`);
- `steering`, whose `--follow` prints one JSON line per settled steering batch,
  which makes it an NDJSON event stream already (`src/main.rs:2753-2766`);
- `admin inspect`, `admin list`, `admin detect`, and `admin dashboard`
  (`src/main.rs:3734-3769`);
- `admin queue pause`, `admin queue resume`, and `admin queue drain`
  (`src/main.rs:3981-4015`);
- `board` (`src/main.rs:2808`), `reliable-sync-status` (`src/main.rs:3524`), and
  `jobs status` (`src/main.rs:4175`).

Only a few payloads carry a `schema_version`: the semantic diff
(`agent-doc-diff/src/semantic.rs:11`, `:163`), the proof ledger
(`agent-doc-workflow-io/src/proof_ledger.rs:49`, `:78`), and the workflow
invariant catalog (`agent-doc-workflow/src/invariants.rs:15`). The rest are
unversioned, and their field sets change with the implementation.

### FFI and packaging

`agent-doc-ffi` exports 31 C functions. They are pure document operations:
component parsing, CRDT merge, replica open/apply/diff, and patch application
(`agent-doc-ffi/src/lib.rs:166-1299`). The main crate's cdylib exports 106 more.
These are editor-host integration hooks such as the editor IPC listener, write
acks, editor op capture, and `agent_doc_ensure_controller_running`
(`src/ffi.rs:331-3706`). The versioning is ad hoc suffixes (for example,
`agent_doc_write_ack_content_for_editor_v2`, `src/ffi.rs:1292`), and the ABI is
loaded in-process by an IDE. It is the wrong layer for automation: a native
library per platform, an in-process crash domain, and editor-role semantics.
The PyPI package is a bootstrap that installs and execs the binary
(`pyproject.toml`, `python/agent_doc_bootstrap/cli.py`); it has no SDK.

### `agent-doc serve`: an HTTP server that already exists

`src/serve.rs:1-40` is a sync `tiny_http` server. It has:

- a loopback default;
- edit and read-only bearer tokens, required for non-loopback binds
  (`src/serve.rs:250-280`, `:369`, `:380`);
- optional TLS;
- an SSE stream (`/events`);
- JSON endpoints (`/api/sessions`, `/api/board`).

It already fits the no-async rule (`AGENTS.md:14`). Its write path is a warning
for any HTTP facade: `POST /save` takes a whole-document body. With no response
cycle open, it calls `std::fs::write(&doc, &body)` and then `git commit`
(`src/serve.rs:1135-1150`). Only the open-cycle branch goes through
`atomic_write_through_authority` (`src/serve.rs:1177-1178`). A whole-document
write from outside the editor, on the no-cycle branch, is the exact class the
editor authority rules forbid. See [Follow-ups](#follow-ups-found-while-mapping).

## 1. What should "an API for automation across all languages" mean here?

**Callers**, from most to least concrete:

- **External event sources** that hand work to a session: CI watchers, test
  runners, game-engine play-mode bridges, issue and PR bots. This is the
  `queue add` population.
- **Other agents and orchestrators**, which enqueue work into a session, wait
  for it, and read the result.
- **Dashboards and status surfaces**: status bars, the fleet board, tmux status
  lines, notification relays. They read state and want push events.
- **Scripts**: shell, Python, or Node one-offs that read a component or check
  whether a session is idle.

**Operations**, grouped by risk:

| Group | Operation | Backing today |
|---|---|---|
| Discover | `api.capabilities`: API version, supported ops, binary version, project root | `version --json`, controller `status` |
| Discover | `documents.list`: session documents in the project | `/api/sessions` (`serve`), `admin list --json` |
| Read | `document.read`: realtime current text, whole or one component | `read [--component]`, controller `crdt_current_text` |
| Read | `document.status`: actor state, claim/owner pane, open cycle, queue active, queue head, last receipt | `admin inspect --json`, `document_state_projection`, `turn_admission_projection` |
| Write (intent) | `queue.add`: attributed, keyed, idempotent | `queue add --json` |
| Write (control) | `queue.control`: pause, resume, drain | `admin queue pause/resume/drain --json`, controller `queue_control` |
| Observe | `events.subscribe`: cycle opened/closed, queue changed, claim changed, steering batch, controller handoff, with a resumable cursor | `steering --follow`, `state_subscribe` (`document_version` cursor, `rpc.rs:22735-22750`), `state_plane_subscribe` |

Two rules bound the write group. They are what makes the API safe to hand to
other programs:

- **No document-text write in v1.** There is no "set text" and no "replace
  component". Every write is a typed intent that the binary plans against the
  realtime current document and applies through the editor authority. This is
  `queue add`'s path, and `queue add` is the template. Operator text is never
  clobbered (`SPEC.md:44-51`, `#retainedprojectionclobber`). A write that would
  drop operator bytes is refused with a typed error, never applied.
- **No "run a turn" op in v1.** An automation client asks for work by
  enqueueing it. The controller's dispatch and queue scheduler decide when it
  runs. Exposing `dispatch` or the run route would let a remote program start
  harness processes and race the scheduler. The 08b dispatch-receipt rules exist
  to keep that path single-owner (`08b:873-921`).

## 2. Should the controller be the API server?

**As the authority: yes. As the published protocol: no.**

For it:

- It is already the single per-project authority and serializer: one controller
  per root (`08b:12-19`), and "IPC remains the boundary between external callers
  and the controller process" (`08b:18`).
- Every write that must respect the editor goes through the CRDT relay it owns.
  An API outside the controller would have to call back into it anyway.
- Two non-Rust clients already speak its wire.
- It already has push primitives (`document_turn_authority_stream`,
  `state_subscribe`, `state_plane_subscribe`) and receipts (`dispatch_attempts`,
  `admin_operations`, `CausalReceipt`).

Against exposing it as-is:

- **Churn.** 106 commands, a `pub(crate)` request type, typed payloads hidden in
  `diagnostic_payload` strings, and 50 commits a month. Publishing it would
  freeze an internal protocol or break clients constantly.
- **Lockstep versioning.** Write commands demand an exact binary match
  (`rpc.rs:22695`). Every install would break an external client until it was
  rebuilt. The only escape is omitting the version, which silently skips the
  check (`rpc.rs:21823`). Both are bad contracts.
- **Ephemeral endpoint.** The controller launches lazily, retires when idle, and
  hands off on install (`08b:729-760`). A client must handle "no socket yet",
  "socket of a predecessor", and "stream ended, reconnect". Today only
  first-party code knows those rules.
- **Local socket only.** No remote callers, and deep roots overflow `sun_path`
  (`paths.rs:19-50`). That is fine for local automation; HTTP has to be layered
  on top.
- **No auth.** `specs/10-security.md:5` is honest that there is none. Anything
  that can open the socket can drive every command, including `admin_control`
  and `shutdown`. A public API must not inherit "all or nothing".

So the controller should *host* a new, narrow command family whose request
schema, error codes, and version are owned separately from the internal 106.

## 3. Options

### (a) A versioned public subset of the controller RPC on its socket, plus a schema and thin clients

One new controller command, `api`, carries `{"api":"agent-doc/v1","op":"queue.add","request_id":…,"idempotency_key":…,"params":{…}}`
and returns
`{"api":"agent-doc/v1","ok":…,"data":…,"error":{"code":"…","message":"…","retryable":…}}`.
The ops dispatch to the same functions the CLI uses.

| Aspect | Assessment |
|---|---|
| Security | Local-only. Harden by creating `.agent-doc/` with mode `0700` and the socket with `0600`, and by checking peer credentials (`SO_PEERCRED` / `getpeereid`) for the same uid. Read-only versus mutating ops become a per-op property, so a later token scope can map onto them. |
| Stability | Semantic versioning per API major, independent of the binary version. `api` is exempt from the exact-binary gate and checked against the declared `api` version instead. Additive fields within v1; removal needs v2. Stable error `code`s. |
| Streaming | One long-lived op, `events.subscribe`, on the same connection, NDJSON. Modeled on `document_turn_authority_stream`, with a `document_version` cursor so it resumes across controller handoff. |
| CRDT authority | Best of all the options: the handlers run *inside* the authority and reuse `converge_document_or_disk`. |
| Cost | Every language needs a socket client, plus the launch, retire, and handoff rules. Small (about 100 lines in Python or TypeScript) but nonzero. Windows named-pipe parity rides on `interprocess`. |

### (b) An HTTP, JSON-RPC, or gRPC facade, as a separate daemon or a controller mode

| Aspect | Assessment |
|---|---|
| Security | HTTP is reachable from any process, including browsers (CSRF and DNS rebinding on loopback). It needs tokens even on loopback, an Origin and Host check, and TLS for remote use. `serve` already has edit and read-only tokens and TLS (`src/serve.rs:250-280`). Multi-user would need per-user tokens and per-document authorization, which does not exist (`specs/10-security.md:24-25`). |
| Stability | HTTP paths version cleanly (`/api/v1/...`). gRPC gives codegen but needs HTTP/2, protoc toolchains, and an async stack, which conflicts with the no-async rule (`AGENTS.md:14`) and is heavy for scripts. |
| Streaming | SSE is simple and already in `serve`. WebSockets are not needed. |
| CRDT authority | As a *separate daemon*, it must forward every write to the controller. It becomes a second process with its own lifecycle, which must not cache state or write disk (see `serve`'s raw write). As a *controller mode*, it ties a network listener to an endpoint that retires and hands off, so the port would move or drop on every install. |
| Cost | Highest. Best reach: dashboards, remote machines, browser UIs. |

Verdict: as a **facade inside `agent-doc serve`** that calls the same op table
through the controller, yes, later (Phase 4). As a new daemon, or as gRPC, no.

### (c) Standardize the CLI `--json` contract plus `queue add`, with stable schemas

`agent-doc api <op> [--json]` reads the request from flags or stdin and prints
the v1 response envelope. The exit code distinguishes `ok`, `refused` (a typed
domain error), and `transport` (retryable).

| Aspect | Assessment |
|---|---|
| Security | Inherits the caller's OS identity and filesystem permissions, with nothing new exposed. |
| Stability | Same v1 schema as (a). The CLI already re-execs a fresh binary and knows the controller launch and handoff rules. That complexity stays in Rust, and callers never see it. |
| Streaming | `agent-doc api events --follow` as NDJSON on stdout, like `steering --follow` today. |
| CRDT authority | Same as (a), since it is the same handlers. |
| Cost | Lowest. It works from every language, CI system, and Makefile on day one. Process spawn costs tens of milliseconds, which is fine for automation and wrong for keystroke-rate callers (editors keep their own wire). |

### (d) MCP as the cross-language API

| Aspect | Assessment |
|---|---|
| Security | stdio, a child process per client, the client's identity. MCP HTTP transports would bring their own auth story. |
| Stability | The MCP protocol version is negotiated (`src/mcp.rs:7`), but tool schemas are not versioned: tool descriptions are LLM-facing prose. |
| Streaming | Notifications exist, but most MCP clients are LLM hosts that don't consume push events. |
| CRDT authority | Fine, since tools shell into the binary (`src/mcp.rs:566`). |
| Cost | Low for agent hosts, awkward for everything else: a CI job or dashboard has to speak JSON-RPC session negotiation to enqueue one line. |

Verdict: MCP is the right surface for *agents in a harness*. It should expose
the v1 ops as generated tools (`agent_doc_queue_add`, `agent_doc_status`, …).
It is a consumer of the API, not the API.

## 4. Recommendation and phased plan

Use **(c) first, then (a), then optionally (b) as a `serve` facade, plus (d) as
generated tools.** All four share one operation table and one schema, so there
is one API with four transports, not four APIs.

**Phase 0: decide (this proposal).** The operator answers the open questions
below.

**Phase 1 (MVP): `agent-doc api`, CLI transport, read and enqueue only.**

- Add a pure crate, `agent-doc-api`, holding the v1 request, response, and error
  types, the op table, and the JSON Schema output, written to `schemas/api/v1/*.json`
  by a generator and checked by `audit-docs` or a test.
- Ops: `api.capabilities`, `documents.list`, `document.read` (realtime current,
  never a stale disk read while an editor holds the document), `document.status`,
  and `queue.add`. `queue.add` is a re-export of the `#queueinbound` path with
  `idempotency_key` mapped to `--key`, so it adds no new write path.
- Errors carry a stable `code` (`not_a_session_document`, `component_not_found`,
  `operator_text_loss_refused`, `duplicate`, `controller_unavailable`,
  `api_version_unsupported`, …) and `retryable`.
- Spec section in `specs/07-orchestration-commands.md`, conformance fixtures
  under `conformance/`, and tests that pin each schema and each error code.
- The existing `--json` outputs stay as they are, and are documented as
  diagnostic rather than contract.

**Phase 2: the socket transport and hardening.**

- A controller `api` command carrying the same bytes, exempt from the exact
  binary gate and checked against the `api` major version instead.
- Socket and directory modes, plus a peer-uid check, for *all* controller
  commands, not only `api`.
- `agent-doc api` becomes a thin client of the socket.
- Reference clients in Python and TypeScript (standard library only, about 100
  lines each) plus a shell example, generated from or tested against the schema.
- `queue.control` (pause, resume, drain), mapped onto the 08b `QueueControl`
  path.

**Phase 3: events.**

- `events.subscribe` with a resumable cursor built on the `state_subscribe`
  `document_version` (monotonic across retention, `rpc.rs:22735-22743`).
- Typed events: `cycle.opened`, `cycle.closed`, `queue.changed`,
  `claim.changed`, `steering.batch`, and `controller.handoff` (reconnect with
  the cursor).
- Delivered as NDJSON on the socket and through `agent-doc api events --follow`.
- A subscriber counts as an `active_client` (`08b:758`). Decide whether
  subscribers should keep a controller alive (see the open questions).

**Phase 4 (optional, demand-driven): HTTP/SSE and MCP.**

- `agent-doc serve` gains `/api/v1/<op>` and `/api/v1/events` (SSE). They call
  the op table through the controller and reuse the bearer tokens and TLS,
  mapping the read-only token to read ops.
- Add Origin and Host checks for loopback.
- MCP gains generated tools for the read, enqueue, and status ops.

**Invariants every phase must keep:**

- the API never writes disk behind an editor;
- the API never writes whole-document or component text in v1;
- every write is attributed (`from:<source>`) and idempotent;
- every write that would drop operator bytes is refused with a typed error;
- the API never starts harness processes directly.

## Open questions for the operator

1. **Who is the first real caller?** A CI watcher, a dashboard, another agent,
   or a game-engine bridge? This decides whether Phase 1 needs events or only
   enqueue and status.
2. **Local-only, or remote and multi-user?** If local-only, Phases 1-3 are
   enough and HTTP can wait. If remote or multi-user is in scope, per-user
   tokens and per-document authorization need a security spec first
   (`specs/10-security.md` has none).
3. **Is "start a turn now" ever an API op?** Or does automation only enqueue,
   with queue activation and the scheduler deciding when to run? The proposal
   says enqueue-only.
4. **Will automation ever write document text?** For example, appending a
   status block or replacing a component it owns. If yes, which components can
   an external writer own, and how is that ownership declared in the document?
5. **What should the public envelope be?** Plain JSON (proposed, with fields
   that map 1:1 onto Lazily `CommandSubmit`: `idempotency_key`, generation
   guard, receipt), or the Lazily `CommandSubmit` envelope itself, which gives
   receipts and handoff forwarding for free but requires a Lazily binding per
   client language?
6. **Should an API call launch a controller?** Writes need one. Should reads
   fall back to a cold read when no controller is running, and should event
   subscribers keep a controller from retiring?
7. **What stability promise for v1?** How long is a deprecation window, and is
   the API version reported independently of the 0.35.x binary version?
8. **Is there a project-spanning gateway?** One user-level endpoint across
   project roots, for a fleet dashboard, or one endpoint per project as
   proposed?

## Follow-ups found while mapping

These are independent of the direction chosen:

- **`serve` raw disk write.** `POST /save` with no open cycle writes the whole
  body with `std::fs::write` and commits (`src/serve.rs:1135-1150`). That
  bypasses `atomic_write_through_authority` and the CRDT relay while an editor
  may hold the document. Route it through the authority, or refuse it while an
  editor replica is attached.
- **Controller socket hardening.** The controller sources have no socket or
  directory mode and no peer-credential check. Also, a request without
  `binary_version` skips the stale-binary refusal for write-bearing commands
  (`rpc.rs:21823-21833`).
- **Spec drift.** `specs/10-security.md:10` says "No network services are
  exposed", but `agent-doc serve` can bind non-loopback addresses with bearer
  tokens (`src/serve.rs:25-26`).
