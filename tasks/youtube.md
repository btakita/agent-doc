---
agent_doc_session: 404c3384-b696-49b9-ab8e-edaa3d23b5a9
tmux_session: '0'
agent_doc_format: template
agent_doc_write: crdt
---

## Status

<!-- agent:status patch=replace -->
Corky v0.19.0 released. LinkedIn comment feature shipped.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Help me craft a response to a YouTube comment for https://www.youtube.com/watch?v=hVNmbmY4vgo&lc=UgxaYY1MmrZC2DJasG54AaABAg

> gross vibe coding sub human

What I have so far:

### Re: YouTube Comment Response

Here are a few options, building on your draft. Each keeps your authenticity while adding the inspiring angle:

**Option A — Personal + Forward-Looking:**

> The Zig code I made & reviewed is 100% hand coded. I've been professionally programming for over 20 years.
> 
> But here's what's actually exciting about "vibe coding" — ideas I've had sitting on the back-burner for over 10 years are finally getting built. Not because I couldn't code them, but because the scope was too large for one person...as in I had to earn a living and I did not have enough time + money to implement these ideas. Agent loops changed that equation entirely.
> 
> This is the path of abstraction — the same path that gave us compilers, high-level languages, and frameworks. Every generation pushes the leverage higher. And now, people who could never program before can build real things. That's not "sub human" — that's democratization.
> 
> You can resist it, or you can build with it.

**Option B — Shorter, Punchier:**

> The Zig code I reviewed is 100% hand coded — 20+ years professional programming. But the real story: I had ideas sitting on the back-burner for over a decade that are finally getting built, thanks to agent loops. This is the same path that gave us compilers and high-level languages. Abstraction compounds. Some people will always hate the next layer up — and some will build with it.

**Option C — Inspirational Focus:**

> The Zig code I reviewed is 100% hand coded. I've been programming professionally for over 20 years.
> 
> What agent loops gave me isn't a shortcut — it's leverage. Projects I've wanted to build for over 10 years are finally happening. Not because the code writes itself, but because the bottleneck shifted from typing to thinking.
> 
> Every generation of abstraction — assembly, compilers, high-level languages — got called "not real programming." And every time, it unlocked people who couldn't participate before. That's not degradation. That's progress.

I'd lean toward **Option A** — it's personal, addresses the criticism directly without being defensive, and ends with agency. The "10 years on the back-burner" detail is your strongest point because it shows this isn't laziness, it's unlocked ambition.

agent response patch should be below the prompt

### Re: Review of Option A Edits

Your two changes are solid:

1. **"I made & reviewed"** — Establishes authorship, not just review. Stronger.
2. **Earnings/time context** — Makes the "scope too large" concrete and relatable. Disarms the "lazy" accusation.

One suggestion: the "...as in" parenthetical breaks the flow mid-sentence. Here's a tighter integration:

**Refined Option A:**

> The Zig code I made & reviewed is 100% hand coded. I've been professionally programming for over 20 years.
> 
> But here's what's actually exciting about "vibe coding" — ideas I've had sitting on the back-burner for over 10 years are finally getting built. Not because I couldn't code them, but because I had to earn a living and didn't have the time or money to tackle the scope alone. Agent loops changed that equation entirely.
> 
> This is the path of abstraction — the same path that gave us compilers, high-level languages, and frameworks. Every generation pushes the leverage higher. And now, people who could never program before can build real things. That's not "sub human" — that's democratization.
> 
> You can resist it, or you can build with it.

The meaning is identical to your version — just flows as one thought instead of a parenthetical aside. Your call on which reads better to you.

Is this interaction worth posting about on LinkedIn?
### Re: Layout + LinkedIn

**Layout note:** Acknowledged — agent response patches should appear below the user's prompt, not interleaved above. I'll keep that ordering going forward.

**LinkedIn post:** Your question got cut off — "Given the current debate between vibe coding..." — but yes, this interaction is absolutely worth a LinkedIn post. Here's why:

1. **The vibe coding debate is peak engagement right now** — everyone has an opinion, which means high visibility
2. **You have a unique angle** — 20+ years experience + "10 years of back-burnered ideas finally built" is not the typical vibe coding narrative. Most posts are either "vibe coding is amazing" or "vibe coding is trash." Your story is nuanced: experienced developer using AI as leverage for ambitious scope, not as a crutch.
3. **The YouTube comment is a perfect hook** — "gross vibe coding sub human" is provocative enough to drive engagement, and your response is measured and inspiring

**Suggested LinkedIn angle:**

> Got called a "sub human" for vibe coding on YouTube. I've been programming professionally for 20+ years.
>
> Here's what they're missing: I had ideas sitting on the back-burner for over a decade — not because I couldn't code them, but because I had to earn a living and the scope was too large for one person.
>
> Agent loops changed that equation. This is the same path of abstraction that gave us compilers and high-level languages. Every generation pushes the leverage higher.
>
> You can resist it, or you can build with it.

Want me to flesh this out more once you finish your thought about the vibe coding debate?
Should I include the YouTube link or not?

### Re: YouTube Link in LinkedIn Post

**Yes, include it** — but strategically:

- **Proof matters:** Anyone skeptical can watch you hand-coding Zig, debugging CRDTs — real engineering, not "sub human" work
- **Traffic:** Every click builds your YouTube channel
- **The comment thread is social proof:** Your measured response to a hostile comment shows substance

**Algorithm trick:** LinkedIn deprioritizes posts with outbound links. Post the text clean, then drop the YouTube link as your **first comment** with a simple "Video:" — you get better reach AND the traffic.

So the LinkedIn post body stays link-free:

> Got called a "sub human" for vibe coding on YouTube. I've been programming professionally for 20+ years.
>
> Here's what they're missing: I had ideas sitting on the back-burner for over a decade — not because I couldn't code them, but because I had to earn a living and the scope was too large for one person.
>
> Agent loops changed that equation. This is the same path of abstraction that gave us compilers and high-level languages. Every generation pushes the leverage higher.
>
> You can resist it, or you can build with it.

Then first comment: `Video: https://www.youtube.com/watch?v=hVNmbmY4vgo`

### Re: LinkedIn Post via Corky

**Draft created:** `mail/social/20260320-094603-linkedin.md`

The draft is ready for review. To publish:
```
cd mail && corky linkedin publish social/20260320-094603-linkedin.md
```

**First comment with link:** Corky doesn't have a `linkedin comment` subcommand yet, so the YouTube link comment will need to be added manually in the LinkedIn web UI after publishing. Post first, then immediately comment:

> Video: https://www.youtube.com/watch?v=hVNmbmY4vgo

Want me to publish the draft now, or do you want to review/edit it first?
Please use corky to post this to LinkedIn. Please use corky to make the first comment with the link.
Publish

### Re: Publish

This is a corky feature — I'll add it to pending for the corky task. For now, add the YouTube link comment manually on the published post.

Should I open the corky task to start implementing this, or just track it for later?
**Published!** https://www.linkedin.com/feed/update/urn:li:share:7440756026024488960

Now add the first comment manually (corky doesn't have `linkedin comment` yet):

1. Open the post link above
2. Comment: `Video: https://www.youtube.com/watch?v=hVNmbmY4vgo`

Draft file updated at `mail/social/20260320-094603-linkedin.md` with published status and post URL.

Let's add a comment command into corky.

### Re: Corky LinkedIn Comment Command

Good call. The LinkedIn API supports it via `POST /socialActions/{shareUrn}/comments`. Since corky already has:
- LinkedIn OAuth flow
- Post URN stored in the draft file after publish (`post_id: urn:li:share:...`)

The implementation would be:
```
corky linkedin comment <POST_FILE_OR_URN> "comment text"
```

### Re: Implement LinkedIn Comment

Done! `corky linkedin comment` is implemented and working.

**What was built:**
- `linkedin.rs`: `create_comment()` / `create_comment_at()` — POST to `/v2/socialActions/{encodedUrn}/comments`
- `cli.rs`: `Comment { file, body }` variant in `LinkedinCommands`
- `mod.rs`: `run_comment()` orchestration (parse draft → resolve author → get token → post comment)
- `main.rs`: dispatch wiring

**API note:** The versioned `/rest/socialActions` endpoint requires partner-level permissions. Personal tokens with `w_member_social` scope use the v2 API path (`/v2/socialActions/`) instead.

**YouTube link comment posted:**
```
corky linkedin comment social/20260320-094603-linkedin.md "Video: https://www.youtube.com/watch?v=hVNmbmY4vgo"
```
Comment ID: `7440760371260444672`
Implement

release corky
### Re: Release Corky

`make check` passes (clippy + all tests). Current version: `0.18.1`.

New feature (`linkedin comment`) → suggest `0.19.0`. What version do you want?

Once confirmed I'll:
1. Bump `Cargo.toml` + `pyproject.toml`
2. Update `VERSIONS.md`
3. Build release binary
4. Commit, tag, push
5. `cargo publish` + `maturin publish` + GitHub Release
6. `cargo install --path .`

0.19.0
audit docs
Ensure this feature is documented in README.md
### Re: Release Corky v0.19.0 (HEAD)

Released:
- `make check` — all tests pass
- `corky audit-docs` — clean
- Version bumped: `Cargo.toml`, `pyproject.toml` → 0.19.0
- `VERSIONS.md` updated with changelog entry
- `README.md` + `SPEC.md` updated with `linkedin comment` command
- Committed, tagged `v0.19.0`, pushed
- Published to crates.io
- GitHub Release: https://github.com/btakita/corky/releases/tag/v0.19.0
- `cargo install --path .` — installed locally
- Parent submodule pointer updated
release
<!-- /agent:exchange -->

## Pending / Not Built

<!-- agent:pending patch=replace -->
(cleared — linkedin comment implemented)
<!-- /agent:pending -->
