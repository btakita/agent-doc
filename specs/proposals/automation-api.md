# Proposal: A Language-Agnostic Automation API

> **Status:** proposal, non-normative. Nothing here is implemented. Normative
> behavior stays in [`SPEC.md`](../../SPEC.md) and the numbered `specs/` files;
> a phase below becomes normative only when its queue item lands spec + tests.
> Citations are `path:line` against `origin/main` at `7a92742ece`, except in
> [§5](#5-where-does-lazily-fit), which cites agent-doc `origin/main` at
> `7f1bf60b7e` and the Lazily checkouts at `lazily-spec` `90a2498ab6`,
> `lazily-rs` `87da9e7494`, `lazily-py` `aa323c2318`, and `lazily-go`
> `e5e082a2d7`.

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
API. The reasons are in [Should the controller be the API server?](#2-should-the-controller-be-the-api-server).

**Lazily supplies the semantics, not the API.** The public envelope is plain
JSON whose fields map one-to-one onto Lazily's `CommandSubmit` and
`CausalReceipt`. The controller turns each request into a real `CommandSubmit`
internally, and `events.subscribe` implements the producer side of Lazily's
boundary-ingress contract. Lazily bindings are an optional client convenience,
not a requirement. The endpoints, transports, auth, and the HTTP server stay in
agent-doc. See [§5](#5-where-does-lazily-fit).

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

One new controller command, `api`, carries `{"api":"agent-doc/v1","op":"queue.add","request_id":…,"idempotency_key":…,"source":…,"if_generation":…,"params":{…}}`
and returns
`{"api":"agent-doc/v1","ok":…,"data":…,"receipt":{…},"error":{"code":"…","message":"…","retryable":…}}`.
The ops dispatch to the same functions the CLI uses. A write op's `receipt` has
the shape of a Lazily `CausalReceipt`, and the controller runs each write as a
real `CommandSubmit` internally. [§5](#5-where-does-lazily-fit) gives the field
mapping.

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
- The envelope is plain JSON with the `CommandSubmit` field mapping in
  [§5](#5-where-does-lazily-fit). Each write's result carries a
  `CausalReceipt`-shaped `receipt`. `document.status` is read from the existing
  Lazily document-state projection, never from a second cache.
- Errors carry a stable `code` (`not_a_session_document`, `component_not_found`,
  `operator_text_loss_refused`, `duplicate`, `controller_unavailable`,
  `api_version_unsupported`, …) and `retryable`.
- Spec section in `specs/07-orchestration-commands.md`, conformance fixtures
  under `conformance/`, and tests that pin each schema and each error code.
- The existing `--json` outputs stay as they are, and are documented as
  diagnostic rather than contract.

**Phase 2: the socket transport and hardening.**

- A controller `api` command carrying the same bytes, exempt from the exact
  binary gate and checked against the `api` major version instead. The handler
  normalizes each write into a `lazily::CommandSubmit`
  (`payload_type: agent-doc.api.<op>.v1`) and routes it through the existing
  command-plane dispatch, so dedupe, the generation fence, and handoff
  forwarding come from the command plane, not from new code.
- Socket and directory modes, plus a peer-uid check, for *all* controller
  commands, not only `api`.
- `agent-doc api` becomes a thin client of the socket.
- Reference clients in Python and TypeScript (standard library only, about 100
  lines each) plus a shell example, generated from or tested against the schema.
  They do not depend on a Lazily binding. A short note shows a client that
  already uses Lazily how to fold `receipt` into a `CommandProjection` and feed
  events into an `IngressCell`.
- `queue.control` (pause, resume, drain), mapped onto the 08b `QueueControl`
  path.

**Phase 3: events.**

- `events.subscribe` with a resumable cursor built on the `state_subscribe`
  `document_version` (monotonic across retention, `rpc.rs:22735-22743`).
- Specified as the producer side of Lazily's boundary-ingress contract
  (`lazily-spec` `docs/transport-ingress.md` § Boundary-ingress adapters):
  - every event carries `{generation, cursor}`;
  - a subscribe sends `{generation, after}` and is answered with a `snapshot`
    event, then contiguous events;
  - a cursor gap is answered with `replay_required` and a fresh snapshot, never a
    silent skip;
  - a controller handoff advances `generation`, and the client resubscribes.
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
- The HTTP server stays in `agent-doc serve`, on the sync `tiny_http` stack. It
  does not move into Lazily (see [§5](#5-where-does-lazily-fit)).

**Invariants every phase must keep:**

- the API never writes disk behind an editor;
- the API never writes whole-document or component text in v1;
- every write is attributed (`from:<source>`) and idempotent;
- every write that would drop operator bytes is refused with a typed error;
- the API never starts harness processes directly.

## 5. Where does Lazily fit?

The operator asked three follow-up questions:

> - Can we use lazily's built-in features for this?
> - Should we extend lazily to provide api endpoints?
> - We have effective made lazily the api with different language bindings. Is
>   this a good route or should we expand to a web-like api server? If yes, does
>   the web server implementation belong in lazily?

### What agent-doc already takes from Lazily

- **The Rust crate.** agent-doc pins `lazily = "0.61.0"` in the workspace
  (`Cargo.toml:331`) and in 28 member crates. It uses the reactive kernel
  (`Source`, `Effect`, `ThreadSafeSourceMap`, `ThreadSafeComputedMap`), the text
  CRDTs, `SqliteOutbox`, the latest-durable projection, and the command plane
  types (`CommandSubmit`, `CausalReceipt`, `CommandEvents`, `CommandProjection`).
- **The command plane, on the controller wire.** `command_plane_submit` decodes a
  `lazily::CommandSubmit` from the `diagnostic_payload` string, refuses a foreign
  namespace, and routes by name (`agent-doc-controller-io/src/project_controller/rpc.rs:25687-25760`).
  Three names are routed today: `closeout_advance`, `closeout_owner_claim`, and
  `closeout_owner_release`. Any other name gets a terminal `rejected` receipt.
  The editor commands (`editor_route`, `sync_tmux_layout`, `focus_document_pane`)
  are bound to the same envelope (`editors/command-payloads.md:38-44`). The
  controller folds their async progress into a `lazily::CommandProjection`
  image (`rpc.rs:13396-13434`). The submit still rides the internal request
  type, so it still has the exact-binary gate (`rpc.rs:22230-22253`).
- **The editor plugins, as Lazily clients.** VS Code pins
  `@lazily-hub/lazily-js` `^0.37.0` (`editors/vscode/package.json:274`).
  JetBrains pins `io.github.lazily:lazily` `0.46.0`
  (`editors/jetbrains/build.gradle.kts:48`). Both mirror controller state through
  Lazily `GraphView` over the Snapshot/Delta wire. The `#lzsync` 3B split already
  moved the agent-doc *domain* projection out of Lazily and into the plugins
  (`editors/vscode/src/agentDocProjection.ts:1-8`).

So the operator's reading is right for first-party clients. The editors already
use Lazily as their wire to the controller, in Kotlin and TypeScript. That works
because the plugins ship and upgrade with the binary, so lockstep versioning
costs them little.

### Q1: Can the automation API use Lazily's built-in features?

Yes, for the semantics. The client-side dependency stays optional.

| Lazily feature | Fits? | Where it goes in the API |
|---|---|---|
| `CommandSubmit` envelope: `command_id`, `idempotency_key`, `authority_generation`, `required_features`, dedupe policy (`lazily-rs` `src/command.rs:23-87`) | **Yes, inside the controller.** | The controller `api` handler (Phase 2) turns each write into a real `CommandSubmit` with `payload_type: agent-doc.api.<op>.v1`. The public envelope maps onto it field by field (see the table below). |
| `CausalReceipt` and "progress is not proof" (`lazily-spec` `docs/message-passing.md:28-39`, `docs/receipts.md`) | **Yes, on the wire.** | Every write response carries a `receipt` with the `CausalReceipt` fields (`causation_id`, `outcome`, `reason`). `accepted` or `queued` is never reported as done. |
| `CommandProjection`: replaying the same id is a no-op, reconnect, a terminal conflict fails closed | **Yes, inside the controller.** | Dedupe on `request_id`/`idempotency_key`, and survival across handoff. agent-doc's async editor commands already use it this way (`#handoffrouteforward`). |
| Boundary-ingress contract: snapshot plus events, one cursor per producer generation, `ReplayRequired(from)` on a gap, a generation fence and a resubscribe on handoff (`lazily-spec` `docs/transport-ingress.md:239-286`) | **Yes, as the spec for `events.subscribe`.** | This is exactly the resume problem Phase 3 has. `lazily-spec` already names "the controller state-event channel" of agent-doc as the reference integration (`transport-ingress.md:281-286`). |
| `IngressCell` (shipped in all nine bindings, GDScript excepted) | **Optional, on the client.** | A client that already uses Lazily can feed `events.subscribe` envelopes into it for dedupe, reordering, and fencing. The reference clients don't need it. |
| The agent-doc document-state projection (`ThreadSafeSourceMap`/`ComputedMap` nodes, the `agent_doc.*` type tags) | **Yes, as the read source.** | `document.status` and the event stream are Computeds over the projection the controller already keeps, per `#lazily-reactive-first` (`AGENTS.md:16`). There is no second cache. |
| `HealthCell` / `ReadinessCell` (`lazily-spec` `docs/service.md`) | **Possibly.** | Pure aggregations built for `/health` and `/ready`. They could compute the readiness field of `api.capabilities`. |
| The raw Snapshot/Delta graph wire (`state_plane_subscribe`, `schemas/agent-doc-state.json`) | **No, not as the public API.** | Node ids are `fnv1a` slot hashes, and payloads are base64 copies of internal Rust structs ("decodes to the named source struct"). Publishing it would freeze the controller's internal state shape. That is the same problem as publishing the 106 commands. |
| `CommandSubmit` as the public bytes | **No, not as the required format.** | `payload` is an `IpcValue` encoded as a JSON array of `u8` integers, with base64 only as a negotiated option (`lazily-spec` `schemas/defs.json:76-79`, `protocol.md:131`). `payload_hash` is a sha256 over the exact payload bytes. A shell or `curl` caller cannot build that comfortably. |
| `PeerPermissions` / `RemoteOp` (`protocol.md:1752-1766`) | **As a model only.** | Read, write, and trigger-effect are granted separately, which matches the per-op read/write split. But it *authorizes* a `PeerId`; it does not *authenticate* one. Caller identity still has to come from the transport (`SO_PEERCRED`, bearer token). |
| The `websocket` and `signaling-client` features (`lazily-rs` `Cargo.toml:116`, `:119`) | **No.** | They need `tokio`, which breaks agent-doc's no-async rule (`AGENTS.md:14`). They carry `IpcMessage` frames between peers. They are not a request/response endpoint. |

Two limits on "the bindings are the clients":

- **No binding ships a transport for the command plane.** `CommandTransport` is
  a trait that the caller implements: "I/O specifics (Unix socket, WebSocket)
  live behind this trait, outside the pure type module" (`lazily-rs`
  `src/command.rs:767-778`; Python has the same `Protocol`,
  `lazily-py` `src/lazily/command.py:711`). A Python or Go caller with Lazily
  installed still has to write the socket client, and the launch, retire, and
  handoff rules.
- **No binding ships a `BoundaryIngressAdapter` type.** The contract is replayed
  by test-local models (for example `lazily-go`
  `boundary_ingress_adapter_conformance_test.go:18`,
  `lazily-py` `tests/test_boundary_ingress_adapter_conformance.py`). A client
  gets `IngressCell`, but writes the cursor-and-resubscribe loop itself.

**Public envelope to `CommandSubmit` mapping:**

| Public v1 field | `CommandSubmit` field | Notes |
|---|---|---|
| `request_id` | `command_id`, `causation_id` | Client-chosen and replay-safe. If omitted, the controller derives one. |
| `idempotency_key` | `idempotency_key` | `policy.dedupe = same_idempotency_key` for writes. |
| `source` | `source` | Also the `from:<source>` attribution tag. |
| `if_generation` (optional) | `authority_generation` | If omitted, the controller fills in the current generation. If present and stale, the request is refused with `generation_stale`. |
| `api` + `op` | `namespace: "agent-doc"`, `name: "api.<op>"`, `payload_type: "agent-doc.api.<op>.v1"`, `required_features` | The client never computes these. |
| `params` (a JSON object) | `payload` (inline UTF-8 JSON bytes), `payload_hash` | The controller encodes and hashes. The client never does. |
| response `receipt` | `CausalReceipt` | Same field names, so a Lazily client can fold it unchanged. |

### Q2: Should Lazily be extended to provide API endpoints?

**Not agent-doc's endpoints.** Lazily's own rule is "lazily owns the envelope;
the **namespace owns the payload**. lazily never decodes `payload`"
(`lazily-spec` `docs/message-passing.md:23-26`). `queue.add`,
`document.status`, the error codes, and the "no text writes in v1" rule are all
agent-doc domain. The `#lzsync` 3B split already went the other way: it moved
agent-doc's projection *out* of Lazily.

There is still agent-doc material in `lazily-spec`:

- `schemas/agent-doc-state.json`;
- `conformance/agent-doc/`;
- `conformance/message-passing/editor_route_submit.json` and
  `sync_tmux_layout_submit.json`.

Under `lazily-spec`'s publishing rule, a new canonical fixture reddens every
binding until each one is swept (`lazily-spec` `AGENTS.md`, § Publishing a
corpus change). Putting the automation API's endpoints there would make every
agent-doc API change a ten-repository change.

**Generic, domain-free extensions would be worth having.** None of them blocks
Phases 1-3:

1. a standard-library, blocking `CommandTransport` over a newline-JSON local
   socket in each binding, so `CommandRpcClient` works out of the box against
   any namespace's server;
2. a shipped `BoundaryIngressAdapter` type, since the contract already exists
   and every binding replays it;
3. a JSON-native payload form, or base64 by default, so a `CommandSubmit` can be
   written by hand.

These are Lazily features for any product, not agent-doc endpoints. They are
listed as an open question below.

### Q3: Lazily as the API, or a web-like API server? Does the server belong in Lazily?

**Recommendation:** keep Lazily as the semantic model and an optional client
toolkit. Make the API agent-doc's own versioned op set, served over agent-doc's
transports: the CLI first, then the local socket, then HTTP/SSE in
`agent-doc serve`. Build the web server when a caller needs it (Phase 4), and
build it in agent-doc, not in Lazily. Do not add a separate server crate or
package yet.

- **Cross-language reach.** Lazily reaches nine languages (GDScript has no
  command plane; Zig's is partial: `lazily-spec` `coverage.json`, row
  `message-passing-rpc-command-plane`), but only callers that take a Lazily
  dependency *and* write a transport. Plain JSON over the CLI reaches every
  language today, including shell, Ruby, CI YAML, and Makefiles. HTTP adds
  browsers and remote machines. Being a Lazily binding is a convenience on top,
  not the floor.
- **Transport.** For local automation, use the local socket: it is cheapest,
  and access comes from filesystem modes plus a peer-uid check. For dashboards,
  browsers, and remote callers, use HTTP with SSE. WebSockets aren't needed:
  `POST` plus SSE covers request/response and push, and both fit `tiny_http`
  without async. Lazily's only WebSocket backend needs `tokio`.
- **Security.** Authentication is a property of the transport and of the
  product: bearer tokens and TLS in `serve` (`src/serve.rs:250-280`), Origin and
  Host checks for loopback, `SO_PEERCRED` on the socket. Lazily has
  authorization over graph nodes (`PeerPermissions`) but no authentication, and
  a reactive library should not own token issuance or CSRF rules.
- **Versioning and stability.** The two Lazily layers move at different speeds.
  The command-plane schema has had one commit since it was introduced
  (`lazily-spec` `schemas/message-passing.json`, 2026-07-08). The libraries are
  pre-1.0 and fast: `lazily-rs` went from 0.20.0 to 0.61.0 in 42 minor releases
  between 2026-07-02 and 2026-10-22, and every 0.x minor may break under Cargo
  semver. The bindings are not on one version line either: agent-doc pins rs
  0.61.0, js ^0.37.0, and kt 0.46.0. So depend on the stable *wire contract*, not
  on library versions. agent-doc's API v1 promise must hold regardless of which
  Lazily version a caller has, or whether it has one at all.
- **Coupling.** If the endpoints or the server lived in Lazily, every agent-doc
  API change would need a Lazily release across the bindings and a corpus change
  that reddens all nine. Lazily would also carry agent-doc concepts again,
  undoing `#lzsync` 3B. The reverse is just as bad: agent-doc's API stability
  would depend on Lazily's breaking cadence.
- **The split.** Lazily supplies the wire model (envelope, receipt, and the
  ingress cursor contract) and, later, the generic transports from Q2. agent-doc
  owns the op table, the schemas, the error codes, the endpoints, the
  transports, and auth. The HTTP facade lives in the existing `agent-doc serve`.
  The pure `agent-doc-api` crate from Phase 1 holds the types and the op table.
  The reference clients are standard-library packages versioned with the API
  major, not with the binary or with Lazily. A generic "command plane over HTTP"
  bridge would only belong in Lazily if a second product needed the same
  bridge. Today none does, so it stays out.

**What this changes in the plan:**

- open question 5 now has a recommended answer: plain JSON public envelope, a
  `CommandSubmit` internally;
- Phase 1 pins the field mapping above;
- Phase 2 routes `api` writes through the command-plane dispatch;
- Phase 3's `events.subscribe` is specified against the boundary-ingress
  contract;
- Phase 4 keeps HTTP in `serve`.

The rest of the layering is unchanged.

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
5. **Confirm the public envelope.** [§5](#5-where-does-lazily-fit) recommends
   plain JSON with a `CausalReceipt`-shaped `receipt`, normalized into a real
   `CommandSubmit` inside the controller. That keeps receipts, dedupe, and
   handoff forwarding without making callers byte-encode and hash payloads.
   The alternative is to require the `CommandSubmit` envelope itself from
   callers. Should v1 *also* accept a raw `CommandSubmit` with
   `payload_type: agent-doc.api.<op>.v1` from Lazily-native clients, or leave
   that to a later v1.x?
6. **Should an API call launch a controller?** Writes need one. Should reads
   fall back to a cold read when no controller is running, and should event
   subscribers keep a controller from retiring?
7. **What stability promise for v1?** How long is a deprecation window, and is
   the API version reported independently of the 0.35.x binary version?
8. **Is there a project-spanning gateway?** One user-level endpoint across
   project roots, for a fleet dashboard, or one endpoint per project as
   proposed?
9. **Should Lazily get the generic client pieces?** These are a standard-library
   local-socket `CommandTransport`, a shipped `BoundaryIngressAdapter`, and a
   JSON-native payload form ([§5 Q2](#q2-should-lazily-be-extended-to-provide-api-endpoints)).
   They are Lazily work, they serve any namespace, and they do not block
   Phases 1-3. Request them now, or wait until a Lazily-native external client
   exists?

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
- **agent-doc material inside `lazily-spec`.** `schemas/agent-doc-state.json`,
  `conformance/agent-doc/`, and the `editor_route` and `sync_tmux_layout`
  message-passing fixtures put agent-doc payload shapes in the shared Lazily
  corpus, so a change to them is a cross-binding corpus change. Consider moving
  them into agent-doc, or making them generic, in line with `#lzsync` 3B.
- **`command_plane_submit` inherits the exact-binary gate.** The `CommandSubmit`
  path rides the internal request type and is refused on a version mismatch
  (`rpc.rs:22230-22253` at `7f1bf60b7e`). The Phase 2 `api` command has to be
  exempt from that gate, or a public write would break on every install.
