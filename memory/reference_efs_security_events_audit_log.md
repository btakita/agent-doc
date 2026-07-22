---
name: reference_efs_security_events_audit_log
description: "EFS security_events table = durable audit log, separate from badge-counted admin feed; threshold email alerts;"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 86681d40-d97f-4925-b4a6-3103dccc4712
---

EFS has TWO admin-signal surfaces — do not conflate:
- `admin_notifications` (migration 0113) = the badge-COUNTED, actionable alert feed
  (`ADMIN_FEED_META` in `lib/admin/notifications/feed.ts`). A guard test
  ("covers exactly the inbound-actionable event set") enforces the curated set to
  avoid fatigue. Do NOT add non-actionable/attacker-triggerable events here.
- `security_events` (migration 0121, `lib/security/events.ts`) = durable, queryable
  audit log for non-actionable security signals that must NOT inflate the badge.
  Writes de-duped per `(type, user_id)` within 5 min (`security_event_record_`).

First producer: enumeration-safe signup collision on an existing VERIFIED account
(`routes.ts` `security_signup_collision_record_`). Threshold email alert to
`MESSAGE_ADMIN_NOTIFICATION_TO` when ≥3 attempts on one account in 24h, throttled
once/account/24h via a companion `signup_existing_account_alerted` event.

Read-only admin panel is LIVE at `/admin/security` (2026-07-09): section 'security'
+ left-nav entry (`layout.tsx`/portal `index.tsx`/`SECTIONS`), `GET
/api/admin/security-events` → `security_events_view_R_` (LEFT JOIN users for
email/role). Read-only; no writes, no schema change beyond mig 0121. Related:
[[reference_efs_users_soft_delete_trigger]] (users are suspended, not deleted).
Verified live via the D1-Edit token [[reference_efs_d1_token_working]].
