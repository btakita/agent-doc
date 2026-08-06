# Example Session — Claude Code

| Field | Value |
|---|---|
| **Name** | Example Session |
| **Source** | example-session.md |
| **Version** | 1.0.0 |
| **Last Updated** | 2026-04-26 |
| **Purpose** | Shows what a real Claude Code interaction looks like — from opening the tool to finished work |
| **Audience** | Robert Ross |

This is a simulated transcript showing what you'll see when you use Claude Code. The **You** lines are what you type; the **Claude** lines are what Claude responds with.

---

## Opening Claude Code

You open your terminal and type `claude` (or open the Claude Code app). You see a prompt:

```
~/Documents/Code $ claude
```

Claude greets you and waits for instructions.

---

## Example: Creating a new project

**You:**
> Create a new project called "build-party-landing" — it's a simple website for the Build Party event.

**Claude:**
```
I'll create a new project for you. Let me set that up.

Creating ~/Documents/Code/src/build-party-landing/
├── index.html
├── styles.css
├── CLAUDE.md

Initializing git repo...
Creating initial commit...
```

**Permission prompt:**
```
Allow Claude to run: git init? (y/n)
```

**You:** y

**Claude:**
```
Done! Project created at src/build-party-landing/ with:
- index.html — basic HTML structure
- styles.css — starter styles
- CLAUDE.md — project description

Want me to add any content to the landing page?
```

---

## Example: Working on an existing project

**You:**
> Open the scoring-api project and add a health check endpoint at /health

**Claude:**
```
Opening src/scoring-api/...

I'll add a health check endpoint. Looking at your project structure...

Your server is in server.js using Express. I'll add a GET /health route
that returns { status: "ok" }.
```

**Permission prompt:**
```
Allow Claude to edit: src/scoring-api/server.js? (y/n)
```

**You:** y

**Claude:**
```
Added GET /health endpoint to server.js. It returns:
  { status: "ok", timestamp: <current time> }

Committing...
```

**Permission prompt:**
```
Allow Claude to run: git commit -m "Add health check endpoint"? (y/n)
```

**You:** y

**Claude:**
```
Committed and pushed. The /health endpoint is live in your repo.

Want me to add a test for it?
```

---

## Example: Checking status

**You:**
> What projects do I have?

**Claude:**
```
Here's what's in your workspace:

src/
├── SessionShare/          (submodule — last pulled 2 hours ago)
├── scoring-api/           (3 commits ahead of remote — needs push)
├── build-party-landing/   (clean — up to date)
└── landing-page/          (2 uncommitted files)

Want me to push scoring-api or commit the landing-page changes?
```

---

## Example: Using a plan

**You:**
> Read my scoring dashboard plan and do the next step

**Claude:**
```
Reading tasks/claudescore/scoring-dashboard-plan.md...

Current status:
- [x] Set up database schema
- [x] Create API endpoints for scores
- [ ] Add WebSocket for live updates  ← next

I'll implement the WebSocket connection for live score updates.
```

*(Claude then writes the code, commits, and checks off the step.)*

---

## Example: Pulling latest changes

**You:**
> Pull all submodules and tell me what's new

**Claude:**
```
Pulling latest for all submodules...

SessionShare: 4 new commits since your last pull
  - "Add rubric for commit quality scoring"
  - "Update SPEC.md with new scoring tiers"
  - "Fix benchmark report formatting"
  - "Add doc-header to specs/README.md"

scoring-api: already up to date
build-party-landing: already up to date

Want me to summarize what changed in SessionShare?
```

---

## Key things to notice

1. **You just talk.** No special commands or syntax needed.
2. **Claude asks permission** before running commands or editing files. You say yes or no.
3. **Claude commits automatically** when it finishes work.
4. **Claude tells you what it's doing** at each step so you're never lost.
5. **You can interrupt or redirect** at any point — just type what you want instead.
