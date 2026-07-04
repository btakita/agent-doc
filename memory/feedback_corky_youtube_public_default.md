---
name: corky YouTube video default privacy
description: Corky should default YouTube uploads to public visibility unless explicitly set to private
type: feedback
---

When uploading YouTube videos via corky, default visibility should be **public** unless the user explicitly sets it to private.

**Why:** Videos were uploaded as private/unlisted, requiring manual fixes in YouTube Studio. The user expects videos to be publicly visible by default.

**How to apply:** When implementing or using corky's YouTube upload feature, ensure the default `privacyStatus` is `public`. Only use `private` or `unlisted` when the user explicitly requests it.
