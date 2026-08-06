---
name: reference_workers_logs_drops_error_header_and_sim_fake_endpoint_false_green
description: "Cloudflare Workers Logs can render an Error as bare stack frames with the header line dropped, and a simulator that implements an endpoint the real service lacks is a permanent false green."
metadata: 
  node_type: memory
  type: reference
  originSessionId: 69693b1f-b95e-4f03-9625-5c75535c64d4
  modified: 2026-07-30T18:21:23.217Z
---

Two findings from EFS `300b788d` (#tmo-sync-cron-failures / #tmo-roster-error-still-silent), each
of which independently hid a production bug for weeks.

**1. `console.error('msg', err)` is not safe in a Worker.** Workers Logs rendered a JSON
`SyntaxError` as its stack frames with the *header line dropped* — production carried
`[tmo-sync] schedule fetch failed 1024` followed by five minified `at async` frames and no
reason at all. The message text was intact (the same string was being written to D1 correctly
in the same code path, which is how it was finally recovered). **Always pass a string:**
`console.error('msg', error_message_(err))`. Do not conclude an error "has no message" because
the log line shows none — check a second sink before believing the log.

**2. A simulator that implements an endpoint the real service does not have is a false green
that never expires.** `lib/tmo-sim` served `LOS.svc/GetLoanPaymentSchedule` with tidy JSON;
real TMO answers it HTTP 404 with a BOM-prefixed XHTML "Endpoint not found." page. Every test
passed while the scheduled sync failed 28 times per batch in production. The tell: **the sim
answered an error case more politely than the real service does.** Make simulators reproduce
the real failure *shape* (status code, content-type, body), not a normalized version of it —
otherwise `await res.json()` before `res.ok` passes in simulation and throws a parser error
against the real host.

Corollary for TMO specifically: `res.json()` must never run before the status check, because a
WCF host answers unknown methods with HTML. See `tmo_read_json_` in `lib/tmo-api/index.ts`.

Related: [[feedback_conformance_tests_drive_real_behavior_not_runner_bookkeeping]],
[[reference_efs_fixed_window_source_grep_tests_break_on_comments]],
[[reference_efs_production_logs_admin_token_and_telemetry_query]].
