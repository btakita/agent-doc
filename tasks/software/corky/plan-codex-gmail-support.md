# 2026-04-27 — `corky` plan for Codex Gmail support

## Goal

Let Codex use Gmail through `corky` without turning `corky` itself into MCP/server infrastructure.

The target architecture is:
- `corky` remains the **library + CLI mail engine**
- a separate Codex-facing adapter/plugin shells out to `corky` or links its library
- Gmail auth, token storage, draft/send/thread logic stay owned by `corky`

## Why this plan exists

Current `corky` already has most of the Gmail primitives:
- auth + token resolution in `src/filter/gmail_auth.rs`
- Gmail sync in `src/sync/gmail_api_sync.rs`
- draft creation / send via Gmail API in `src/draft/mod.rs` and `src/draft/send.rs`
- Google-scope reuse in Docs/Sheets/Calendar/GSC modules

What is missing is a stable, machine-readable surface that a Codex connector can call safely, plus a small amount of Gmail correctness work that should land before Codex depends on it.

## Non-goals

- Do not embed Codex-specific protocol handling inside `corky`
- Do not make `corky` an MCP server just to support Codex
- Do not introduce agent-specific logic into the core draft/send/auth paths unless it benefits the normal CLI too

## Deliverables

- A stable Gmail-oriented CLI contract for connectors
- JSON output for the commands a connector needs
- Remaining Gmail threading/reply correctness fixed before exposing the flow to Codex
- A thin Codex adapter outside `corky` that maps tool calls to `corky`
- Docs covering auth, runtime requirements, and failure modes

## Phase 1: Stabilize Gmail core behavior in `corky`

- [ ] Finish Gmail reply/thread correctness before connector work depends on it
  - `src/sync/markdown.rs`: persist RFC 2822 `Message-ID` metadata needed for replies
  - `src/draft/mod.rs`: stop treating Gmail thread IDs as `In-Reply-To`
  - `src/draft/mod.rs`: include `"threadId"` in Gmail API draft/send requests when replying
- [ ] Audit whether `draft/send.rs` and `draft/mod.rs` should share one Gmail send path instead of duplicating behavior
- [ ] Add focused tests for reply threading so Codex-triggered sends cannot silently fork a conversation

## Phase 2: Add connector-grade CLI surfaces

`corky` needs a small set of commands that are deterministic and JSON-friendly.

- [ ] Add `--json` output to the Gmail commands a connector will actually call
  - `corky draft push ...`
  - `corky draft send ...`
  - `corky sync refetch ...`
  - any thread-inspection or mailbox lookup command the adapter needs
- [ ] Introduce a Gmail auth/status command instead of making the adapter infer state from stderr text
  - likely `corky gmail auth status [--account <name>] [--json]`
  - output should include credential source, token presence, token expiry, granted scopes, and whether re-auth is required
- [ ] Add a machine-readable thread lookup surface if current commands are not enough
  - example: resolve a local conversation file to Gmail thread/message metadata
  - example: fetch the latest thread state before replying
- [ ] Standardize exit codes for connector-relevant failures
  - auth required
  - scope mismatch
  - draft validation failure
  - Gmail API request failure

## Phase 3: Factor shared Google OAuth logic

Right now Gmail-adjacent credential resolution is duplicated across:
- `src/filter/gmail_auth.rs`
- `src/cal/auth.rs`
- `src/gsc/auth.rs`
- `src/contact/push.rs`

Before shipping a Codex adapter, move the shared credential/token resolution rules into one reusable module.

- [ ] Extract shared Google OAuth credential resolution from the duplicated auth modules
- [ ] Keep `client_id`, `client_id_cmd`, env-var fallback, and built-in default behavior identical across features
- [ ] Make scope coverage checks reusable so connector-triggered Gmail flows do not regress into stale-token bugs
- [ ] Extend `corky doctor gmail` to report the exact state a connector would depend on

## Phase 4: Build the Codex adapter outside `corky`

This layer should live outside the core `corky` crate. It can be implemented in `agent-loop`, a Codex plugin, or another thin wrapper repo.

- [ ] Define the minimum tool surface Codex needs
  - create draft
  - send draft
  - inspect auth status
  - find/reload thread context
- [ ] Implement the adapter as a thin wrapper around `corky` CLI commands
- [ ] Keep the adapter stateless except for passing file paths / args / JSON through
- [ ] Do not reimplement Gmail OAuth or token handling in the adapter
- [ ] Document the runtime requirement clearly: Codex must run in an environment with outbound DNS + HTTPS access to Google APIs

## Phase 5: Documentation and release

- [ ] Update `README.md` with a “Codex Gmail support” section that explains the split of responsibilities
- [ ] Update `SPEC.md` with the supported connector-facing CLI contract and JSON schemas
- [ ] Add end-to-end examples for:
  - auth bootstrap
  - draft creation
  - draft send
  - reply into existing thread
- [ ] Add a release note once the CLI contract is stable enough for external adapters

## Suggested implementation order

1. Fix Gmail threading correctness in `draft/mod.rs` and `sync/markdown.rs`
2. Add JSON output + exit-code guarantees to the existing Gmail commands
3. Extract shared Google OAuth helpers
4. Add `corky gmail auth status --json`
5. Build the separate Codex adapter against those CLI surfaces
6. Document the contract in `README.md` and `SPEC.md`

## Risks

- Reply/thread bugs will be harder to notice once an adapter is auto-sending on a user’s behalf
- If the adapter scrapes human-readable CLI output, the integration will be brittle
- Duplicated OAuth logic across Gmail/Calendar/GSC/Contacts will keep drifting unless it is unified first
- Codex runtime networking remains an external dependency even if the adapter is correct

## Success criteria

- A Codex adapter can create and send Gmail drafts through `corky` using only documented CLI/library contracts
- `corky` remains usable as a normal standalone CLI without Codex installed
- Gmail replies preserve correct threading and `Message-ID` semantics
- Auth failures and scope problems are machine-readable and recoverable
