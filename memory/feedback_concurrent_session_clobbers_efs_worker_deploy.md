---
name: feedback_concurrent_session_clobbers_efs_worker_deploy
description: "A concurrent session's `deploy:worker` silently overwrites your EFS Cloudflare Worker deploy, reverting your UI edits live"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: bdaa7518-5470-4c25-ad6f-cefb0ca7c409
---

A concurrent agent session running `bun run deploy:worker` for equityfundingsource
overwrites whatever you last deployed — the live worker is a single shared target,
so the last deploy wins regardless of whose changes it carries.

Observed 2026-07-07: I removed the admin Messages-modal "Open Page" button and
deployed; a concurrent session redeployed an older working tree minutes later,
putting the button back live (verified by string-grepping the fetched browser
bundle — my `messages.tsx` edit was gone but the declarations feature they also
had was present). Bundle content hash on `/` changed from my build to theirs.

**Why:** Cloudflare Worker deploys are not merged — each `deploy:worker` replaces
the running version wholesale. Two sessions building from divergent working trees
race, and neither is warned.

**How to apply:**
- After deploying EFS, VERIFY it stuck: fetch `/`, read the `/index.browser-*.js`
  hash, curl that bundle, and grep for a unique string literal from your change
  (identifiers are minified but string/JSX literals survive). See
  [[reference_efs_verify_deploy_via_string_grep]].
- Expect a transient tiny (~36KB) bundle at `/` mid-propagation right after deploy;
  re-fetch after a few seconds until the full ~1.8MB app bundle hash settles.
- If a concurrent session clobbered you, rebuild from your working tree and
  redeploy; don't assume "I deployed once" means it's live.
- Confirm prod behavior against real data: EFS prod D1 (`280e1539…`, the worker's
  bound DB) is queryable via `bun run cf:d1:query:remote` with the passage
  CLOUDFLARE_D1_TOKEN. [[feedback_concurrent_sessions_force_push_shared_superproject]]
