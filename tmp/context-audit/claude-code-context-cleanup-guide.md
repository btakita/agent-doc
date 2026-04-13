GUIDE

The Claude Code Context
Cleanup Guide

6 fixes that cut your token waste and improve output quality

WHAT YOU'LL LEARN

Understand the Token Tax

Audit Your MCP Servers

Clean Up Your CLAUDE.md

Optimize Your Settings

Fix Your Daily Habits

What's Next

BUILT WITH CLAUDE CODE | 2026

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

01
Understand the Token Tax

Every message you send, Claude rereads the entire conversation from the beginning. Message 30 costs 31x

more than message one. On top of that, your CLAUDE.md, MCP servers, skills, and system prompt all sit in

context for the entire session.

This  means  bloated  context  costs  you  twice:  you  burn  through  your  usage  limit  faster,  and  your  output

quality drops. Models pay the most attention to the beginning and end of context. Everything stuffed in the

middle gets partially ignored.

QUICK CHECK

Run /context in a fresh Claude Code session before typing anything. You'll see your starting token tax.

Mine was over 50,000 tokens before I sent a single message.

KEY INSIGHT

This isn't a limits problem. It's a context hygiene problem. Every fix below targets one source of invisible

waste.

02
Audit Your MCP Servers

MCP servers are the single biggest source of hidden token waste. Every connected server loads all of its tool

definitions  into  your  context  on  every  message,  not  just  when  you  use  a  tool.  One  server  can  eat  18,000

tokens. Four or five unused servers can burn 70,000-90,000 tokens per turn doing absolutely nothing.

The 5-minute fix:

1.

2.

Run /mcp at the start of each session

Disconnect every server you won't use in this session

BUILT WITH CLAUDE CODE | 2026

02

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

3.

Reconnect only when you need them

THE BIGGER MOVE

Replace MCP servers with CLI equivalents wherever possible. A CLI only costs tokens when Claude actually

runs a command. An MCP server costs tokens just by existing in your session.

# Example: instead of keeping Playwright MCP connected,

# use the Playwright CLI through bash commands.

# Same capability, ~40% fewer tokens.

npx playwright test --ui

BASH

PRO TIP

If you use a tool daily, keep the MCP server. If you use it weekly, switch to the CLI. The threshold is frequency:

persistent connections should match persistent needs.

03
Clean Up Your CLAUDE.md

Your CLAUDE.md loads into context at the start of every session and stays there for every API call. But the

real cost isn't just tokens. Anthropic's own engineering team found that too many instructions make output

worse.

It happens to everyone. Bad output, so you add a rule. Another bad output, another rule. Nobody goes back

and removes one. Over time the file gets longer and more contradictory.

Run these five filters on every rule in your CLAUDE.md:

Filter

Question to Ask

Default behavior

Does Claude already do this without
being told?

If Yes...

Cut it

BUILT WITH CLAUDE CODE | 2026

03

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

Contradiction

Does it conflict with another rule?

Pick one, cut the other

Redundancy

Is this already covered elsewhere?

Cut it

Bandaid

Vague

Was this a reaction to one specific
bad output?

Cut it

Would Claude interpret this
differently each time?

Rewrite or cut it

Use  progressive  disclosure.  Your  core  CLAUDE.md  should  only  contain  rules  that  apply  to  every  single

session. Everything else goes into reference files with one-line pointers:

MARKDOWN

# CLAUDE.md (core -- keep this lean)

For API conventions, read docs/api-standards.md.

For testing guidelines, read docs/testing.md.

For deployment rules, read docs/deploy.md.

Claude reads those files on demand. Instead of 5,000 tokens in context the whole session, you get 500 in the

core file and the rest loads only when needed.

04
Optimize Your Settings

Three settings in your settings.json that most people either have wrong or don't have set at all.

1. Autocompact threshold

Claude auto-compacts at around 83% of your context window by default. Output quality degrades before

that. Set it to trigger earlier:

{

  "autocompact_percentage_override": 75

}

JSON

BUILT WITH CLAUDE CODE | 2026

04

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

2. Bash output length

Default bash output is capped between 30,000-50,000 characters. Anything beyond gets silently truncated,

and  Claude  re-runs  the  command  or  asks  for  more  info.  Wasted  tokens  on  retries.  Set  the  environment

variable:

{

  "env": {

    "BASH_MAX_OUTPUT_LENGTH": "150000"

  }

}

3. File permission deny rules

JSON

By default, Claude can read everything in your project. Node modules, dist folders, lock files, build artifacts.
Add deny rules that work like a .gitignore for Claude:

JSON

{

  "permissions": {

    "deny": [

      "Read(node_modules/**)",

      "Read(dist/**)",

      "Read(.next/**)",

      "Read(coverage/**)",

      "Read(*.lock)"

    ]

  }

}

05
Fix Your Daily Habits

Four habits that compound into serious token savings over time.

Start fresh between unrelated tasks. If you just finished 20 messages on content research and switch to
script writing, every scripting message still pays the tax on all that research context. Run /clear between

BUILT WITH CLAUDE CODE | 2026

05

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

tasks. This one habit probably saves more tokens than anything else on this list.

Use plan mode before anything non-trivial. The most expensive mistake in Claude Code is letting it go

down  the  wrong  path.  It  writes  200  lines  of  code,  you  realize  it  misunderstood  the  task,  and  you  scrap

everything. All those tokens are gone. Plan mode lets Claude ask clarifying questions first and get alignment

before writing a single line.

Edit  your  prompt  instead  of  sending  corrections.  When  Claude  gets  something  wrong,  don't  send  a

follow-up.  That  adds  the  bad  response,  your  correction,  and  the  new  response  to  context  permanently.

Instead, go back and edit your original prompt. The bad exchange gets replaced entirely.

Use  the  right  model  for  the  job.  Sonnet  handles  most  coding  work.  Haiku  is  great  for  sub-agents  and

simple  lookups.  Opus  should  only  come  out  for  deep  architectural  planning.  If  most  of  your  tokens  run

through cheaper models, your effective session life goes way up.

What's Next

Your Claude Code setup drifts over time. You add an MCP server here, a rule there, install a new skill. Three

months later your context is full of stuff you don't need anymore.

The fixes in this guide aren't a one-time thing. Run through this list every couple of weeks, or install the free
/usage-audit skill that automates the whole process. It checks your CLAUDE.md rules, settings, skills,

MCP servers, and file permissions. Scores your setup out of 100 and tells you exactly what to cut and why.

Install the /usage-audit skill (takes 10 seconds):

1.

2.

3.

Download the skill folder:
https://drive.google.com/file/d/1KJ0tUHOjBHYZNswaNLF9qrmwgDLT4Q3w/view?usp=drive_link

Unzip and drop the usage-audit folder into your project's .claude/skills/ directory

Start a new Claude Code session and run /usage-audit

your-project/

  .claude/

    skills/

      usage-audit/    <-- drop it here

        SKILL.md

That's  it.  Run  it  once  now  to  get  your  baseline  score.  Run  it  again  in  a  couple  weeks  to  catch  whatever

drifted.

BUILT WITH CLAUDE CODE | 2026

06

THE CLAUDE CODE CONTEXT CLEANUP GUIDE

WANT HELP BUILDING THIS FOR YOUR BUSINESS?

You bring the problem. I'll tell you exactly what to build. 30 minutes, your specific situation, no
generic templates.

cal.com/bradley-bonanno/ai-strategy-call

BUILT WITH CLAUDE CODE | 2026

07

