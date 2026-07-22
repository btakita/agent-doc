---
name: reference_efs_users_soft_delete_trigger
description: EFS prod D1 users cannot be hard-deleted — a trigger forces soft-delete; suspend instead;
metadata: 
  node_type: memory
  type: reference
  originSessionId: 86681d40-d97f-4925-b4a6-3103dccc4712
---

EFS prod D1 `users` has a `SQLITE_CONSTRAINT_TRIGGER` that blocks `DELETE FROM users`:
error `7500 "users must be disabled instead of deleted"`. To retire/clean a user
(e.g. a throwaway signup made while verifying live), soft-delete instead:
`UPDATE users SET status='suspended' WHERE email='...'` (status is 'active' | 'suspended';
the login flow rejects 'suspended' with a 403 "This account is suspended").
Dependent rows (`email_login_tokens`, `notifications`) CAN be hard-deleted by
`user_id` first, but the `users` row itself must be suspended.

Query/edit prod D1 via the D1-Edit token in passage — see [[reference_efs_d1_token_working]].
