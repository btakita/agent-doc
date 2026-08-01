//! # Module: skill
//!
//! ## Spec
//! - Bundles SKILL.md into the binary at compile time via `include_str!`.
//! - `install()` writes the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md`
//!   under the git superproject root (or toplevel if not a submodule).
//! - `install_at(root)` accepts an explicit root override; used by tests.
//! - `install_and_check_updated()` installs and returns `true` if the file was
//!   absent or stale, `false` if already up to date.
//! - `check()` / `check_at(root)` verify the installed skill matches the bundled
//!   version; exit code 1 if out of date.
//! - Install is idempotent: calling it multiple times with identical content is a no-op.
//! - When CWD is inside a git submodule, resolves to the superproject root so that
//!   the skill file lands in the workspace root that Claude Code actually reads.
//! - Claude/Codex/OpenCode installed instructions are rendered from one shared source
//!   surface; only harness-specific invocation wording and frontmatter
//!   description may differ.
//!
//! ## Agentic Contracts
//! - `install()` and `check()` never require arguments; resolution is automatic.
//! - `install_at(Some(path))` is deterministic and safe for isolated tests.
//! - `install_and_check_updated()` is the preferred call site for startup skill sync;
//!   callers can branch on the bool to print "skill updated" notices.
//! - `check_at` exits the process (code 1) rather than returning `Err` when outdated,
//!   making it safe to call from CI scripts.
//!
//! ## Evals
//! - install_creates_file: fresh temp dir → `.claude/skills/agent-doc/SKILL.md` created with bundled content
//! - install_idempotent: install twice → file content unchanged, no error
//! - install_overwrites_outdated: stale "old content" present → replaced with bundled content
//! - check_not_installed: no SKILL.md present → path does not exist (check would return false)
//! - bundled_skill_is_not_empty: shared skill template length > 0 at compile time
//! - bundled_skill_contains_agent_doc: bundled content references "agent-doc"

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;

/// The shared SKILL.md source bundled at build time.
const SKILL_TEMPLATE: &str = include_str!("../SKILL.md");

const CLAUDE_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes /agent-doc <file>. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const CODEX_DESCRIPTION: &str = "Interactive markdown session for Codex. TRIGGER: user writes agent-doc <file> as a normal Codex message. Requires a markdown session document, installed CLI, and write+commit every cycle. Do not use slash commands; Codex rejects project-defined /agent-doc.";

const OPENCODE_DESCRIPTION: &str = "Interactive markdown session for OpenCode. TRIGGER: user invokes /agent-doc <file> command. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const GENERIC_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes the harness-native agent-doc entrypoint. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const CLAUDE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are Claude Code-specific. Other harnesses receive the document path directly.
"#;

const CODEX_INVOCATION_SECTION: &str = r#"## Invocation

Codex does not support project-defined slash commands. Do **not** type `/agent-doc`; the Codex CLI will reject it before these instructions run.

In Codex, invoke agent-doc by writing one of these as a normal message:

```
agent-doc <FILE>
agent-doc claim <FILE>
agent-doc compact <FILE>
agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

Claude Code slash-command equivalents are `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, and `/agent-doc compact exchange <FILE>`.
"#;

const OPENCODE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are installed as an OpenCode command via `agent-doc skill install --harness opencode`. Other harnesses receive the document path directly.
"#;

const GENERIC_INVOCATION_SECTION: &str = r#"## Invocation

Use the harness-native `agent-doc` entrypoint for the environment you are in:

- Claude Code: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`
- Codex: `agent-doc <FILE>`, `agent-doc claim <FILE>`, `agent-doc compact <FILE>`, `agent-doc compact exchange <FILE>`
- OpenCode: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Codex does not support project-defined slash commands. If this root file disagrees with the installed harness-specific instructions, treat the installed harness file as authoritative for the current turn.
"#;

const AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run the active-harness install: Claude Code `agent-doc skill install --harness claude --reload restart` unless `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`; Codex `agent-doc skill install --harness codex`, then re-read the installed `.codex/skills/agent-doc/SKILL.md` completely and continue the same turn without restarting the supervisor; OpenCode `agent-doc skill install --harness opencode`; other harnesses `agent-doc skill install`. If install says already up to date, treat this file as stale duplicate instructions, use installed harness instructions, and continue with the task. Claude stops only on a real `SKILL_RELOAD=...`; see [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const CLAUDE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness claude --reload restart` unless `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`. On `SKILL_RELOAD=restart`, ask the user to restart Claude Code and re-invoke `/agent-doc <FILE>`, then stop. Use `--reload compact` and ask for `/compact` only when that explicit opt-in exists. If already up to date, treat as stale instruction drift, continue this turn, and use the installed Claude skill. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const CODEX_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness codex` without a reload request. After a real update, re-read the installed `.codex/skills/agent-doc/SKILL.md` completely and continue the same turn under those instructions. Do not call `agent-doc session restart-supervisor`, stop, or ask the user to restart: replacing an active Codex child interrupts the conversation, while the explicit re-read loads the updated workflow in place. If install says already up to date, treat it as stale instruction drift, continue this turn, and use the installed Codex instructions. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const OPENCODE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness opencode`; if it says already up to date, treat as stale instruction drift, continue this turn, and use the installed OpenCode skill. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

/// Claude-Code-only auto-loop instruction appended to the rendered SKILL.md.
///
/// Codex has a `Stop` hook in `.codex/hooks.json` that blocks final-answer
/// completion and instructs the active turn to continue the next queue head
/// in-pane when the queue is still active. Claude Code has no equivalent Stop
/// hook, so without this instruction the operator has to retype `agent-doc
/// <FILE>` after every cycle to drain the queue.
///
/// The instruction tells the Claude Code agent to invoke `/loop agent-doc
/// <FILE>` via the `Skill` tool after a successful closeout when the queue is
/// still active. `/loop` drives the next cycle on its self-paced cadence,
/// terminating naturally when the queue drains, when the user types a non-
/// queue prompt, when `agent_doc_queue_max_iterations` is hit, or when the
/// environment hard-cap (`AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP`, default
/// 50) is exceeded.
///
/// Plan: tasks/agent-doc/plan-claude-code-queue-auto-loop.md
const CLAUDE_QUEUE_AUTO_LOOP_SECTION: &str = r#"
## Auto-loop while queue is active (Claude Code)
After a successful `agent-doc finalize` / `agent-doc write --commit` cycle whose `agent-doc session-check` returns OK, check preflight's queue fields:
- `preflight.queue_active == true`
- `preflight.queue_continuation_required == true` — the authoritative "there is agent-drainable work the **in-session loop** can take" signal (`#cleardrainsignal`/`#qchurn`). As of `#qcontdrain` the in-session `/loop` **drains `[clean-session]` heads in place** (it no longer defers them to the supervisor — a supervisor that may itself be stalled left the queue stranded), so this is `false` **only** when every remaining head is `[operator-verify]` (needs a live editor/pane) or inert noise, even while `queue_active == true` and a `queue_trigger` is present. **When it is `false`, do NOT loop and do NOT re-dispatch from this session** — only operator-gated / noise heads remain. When it is `true` and a `[clean-session]` head is next, just drain it; the supervisor still owns context resets between items (idle-boundary recycle), but you no longer stop the loop merely because the head is `[clean-session]`. This signal is computed in the binary from the in-session drainability filter, which defers only `[operator-verify]` heads — the same rule the supervisor idle-watch applies. Never re-derive drainability by hand from the queue prose. `preflight.queue_drainable_head_count` is the underlying loop-scope count (`0` ⇒ stop the loop).
- `preflight.queue_trigger == "auto"` **or** `preflight.queue_trigger == "persisted"` — `auto` is a start trigger only; once the queue is active, a persisted-active queue (`queue_active: true` with no `auto` attribute) is equally continuation-eligible (`#active-queue-persisted-no-continue`). Do not require the `auto` attribute to keep draining an already-active queue.
- `preflight.queue_prompts.len() >= 1`
- `preflight.user_intent_prompt_changes` is empty (a real user prompt mid-loop takes precedence; do NOT auto-loop over it). Managed-component state edits — queue activity toggle, queue item add/strike, backlog/review/done item edits, `queue_active:` frontmatter flip — are filtered out of `user_intent_prompt_changes` so routine session bookkeeping does not block the auto-loop. Likewise, an edit the affectedness classifier scopes as independent of the current turn (`op_affectedness.turn_affected == false`, `#queue-no-stop-unrelated-edit`) is filtered out — an edit unrelated to the active turn never halts the drain; only a real user prompt, which edits the in-scope `exchange` tail and classifies as turn-affecting, preempts.
When all of these hold, first run `agent-doc drain-claim <FILE>` to claim the drain-owner lease, then invoke the `Skill` tool with `skill: "loop"` and `args: "agent-doc <FILE>"` to drive the next cycle from the same Claude Code session. The `drain-claim` step (#kp5z / #qflood) tells the supervisor idle-queue watch that `/loop` owns this drain, so it defers instead of *also* injecting `agent-doc <FILE>` into the input queue and flooding it with duplicate triggers. The lease is short-TTL and self-expiring — if the loop stops, the supervisor resumes draining on its own; you never need to release it manually. `/loop` self-paces the next invocation and terminates naturally when the queue drains, when the user interrupts, when `agent_doc_queue_max_iterations` (frontmatter or `.agent-doc/config.toml`) is hit, or when the environment hard-cap `AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` (default `50`) is exceeded.
**Do not defer a drainable queued item to a "fresh" or "focused cycle" (`#drain-no-defer`).** During an active queue drain, "this deserves its own focused/clean/fully-verified cycle" is a **stall, not valid closeout** — the drain IS the cycle sequence, and context growth between items is the supervisor's job, not a reason for you to stop. When `session_accretion` reports high accretion (`level: warn`/`block`, over `clear_threshold`) while `queue_active == true`, do **not** hand the remaining items back, ask the operator to compact, or pause for a clean session: finish the current item and keep looping. The CP/supervisor resets context between items — its idle-boundary recycle promotes a fresh binary, and the drain re-dispatches the next item to a freshly-`/clear`ed agent so each runs with clean context. You keep finalizing + looping; the supervisor owns the `/clear`. Only a genuinely operator-gated item (a required live editor/pane proof, an external approval/CI outage) may be left for the operator and tagged so go-mode defers it (`#goqueuestall`); an **agent-drainable** item is never deferred to "a fresh cycle."

Skip the auto-loop on any failed closeout, `session-check` interruption, or `lint-gate` block — those need explicit operator attention. Skip when `preflight.queue_active == false` (queue drained or halted) **or `preflight.queue_continuation_required == false`** (no *in-session*-loop-drainable head — `[operator-verify]`, `[focused-cycle]`, or noise heads remain; looping would just churn a no-op `#qchurn` cycle, so report the remaining heads in one line and stop without pausing or punting). As of `#qcontdrain`, `[clean-session]` heads are loop-drainable and therefore keep `queue_continuation_required == true` — do **not** stop the loop on them; drain them in place. High session-accretion is **not** in that skip list — it is handled by the supervisor `/clear`-and-continue path above, not by stopping. **Stopping ≠ stalling — read `ui_outcome` (`#qfocsup`):** `deferred_for_supervisor_drain` (`yield_to_supervisor_clear_and_continue`) means a `[focused-cycle]` head remains that the CP/supervisor clear-and-continue path drains (it force-`/clear`s + re-dispatches to a fresh context — why the in-session loop yields it instead of running mid-drift), so just end your turn for the supervisor to take over and report "yielding to supervisor," never an operator stall (treating it as operator-gated is what wrongly idles the queue); `deferred_for_operator_proof` (`operator_proof_required`) means only `[operator-verify]`/noise heads remain (need a human / clearing) so report and stop; `no_drainable_work` means fully drained.

**The skip list above is exhaustive — do NOT invent stop reasons after a proven editor delivery (`#degraded-ipc-no-stall`).** A closeout is successful only after the monotonic write pipeline reaches `committed` and `session-check` is OK. A timeout, missing Lazily visibility proof, or `recovery=retry_without_disk_write` is not success: do not continue the queue or elect an attached-document disk write. The keyed worker retains the same intent in `state.db` and resumes it after the registered editor endpoint recovers. When `queue_continuation_required == true`, follow the binary's `queue_continuation_guidance`; high session accretion and semantic-completion warnings are not stop reasons after proven closeout. A stale binary recycles at the next idle boundary through the controller projection or `agent-doc admin recycle`, after which the loop continues on the fresh build. `make install` / `agent-doc lib-install` sends the shared `reload_library` intent to every live Lazily editor registration through its PID-scoped endpoint; `agent-doc admin reload-lib` repeats that typed fan-out. No filesystem delivery or reload broadcast participates in the hot path.

This section's `/loop` invocation is Claude-Code-specific. Codex auto-loops via its `Stop` hook in `.codex/hooks.json`. **OpenCode has no `/loop` and no `Stop` hook, so the OpenCode agent MUST self-drive the drain in-turn (`#ocdrainstall` Part A):** after a proven closeout (`agent-doc write --commit <FILE>` reached `committed` + `session-check` OK) with `preflight.queue_continuation_required == true` and `preflight.queue_drainable_head_count > 0`, do NOT end the turn — immediately run `agent-doc preflight <FILE>` again and drain the next head (preflight → respond → `agent-doc write --commit <FILE>`), repeating in the SAME turn until `queue_continuation_required == false`, `queue_drainable_head_count == 0`, a genuine stop condition (failed closeout, `session-check` interruption, only `[operator-verify]` heads remaining), or the `agent_doc_queue_max_iterations` / `AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` (default `50`) cap is reached. "Low context budget" and "this head deserves its own focused/clean cycle" are **NOT** stop reasons (`#drain-no-defer`) — they are stalls; keep draining. If context is genuinely saturated, finish the current item's proven closeout, then end the turn for the supervisor's idle-boundary recycle + re-dispatch — but never stop mid-queue without a proven closeout, and never hand an agent-drainable head back to the operator to re-invoke manually. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) and `tasks/agent-doc/plan-claude-code-queue-auto-loop.md`.
"#;

/// Bundled runbooks installed alongside the skill.
const BUNDLED_RUNBOOKS: &[(&str, &str)] = &[
    (
        "compact-exchange.md",
        include_str!("../runbooks/compact-exchange.md"),
    ),
    (
        "transfer-extract.md",
        include_str!("../runbooks/transfer-extract.md"),
    ),
    ("pending-ops.md", include_str!("../runbooks/pending-ops.md")),
    (
        "model-tier-gate.md",
        include_str!("../runbooks/model-tier-gate.md"),
    ),
    (
        "command-synonyms.md",
        include_str!("../runbooks/command-synonyms.md"),
    ),
    (
        "compound-task-steering.md",
        include_str!("../runbooks/compound-task-steering.md"),
    ),
    (
        "planning-dispatch.md",
        include_str!("../runbooks/planning-dispatch.md"),
    ),
    (
        "streaming-checkpoints.md",
        include_str!("../runbooks/streaming-checkpoints.md"),
    ),
    (
        "document-format.md",
        include_str!("../runbooks/document-format.md"),
    ),
    ("commit.md", include_str!("../runbooks/commit.md")),
    (
        "jb-cache-conflict.md",
        include_str!("../runbooks/jb-cache-conflict.md"),
    ),
    (
        "code-enforced-directives.md",
        include_str!("../runbooks/code-enforced-directives.md"),
    ),
    (
        "harness-invocation.md",
        include_str!("../runbooks/harness-invocation.md"),
    ),
    (
        "dynamic-context.md",
        include_str!("../runbooks/dynamic-context.md"),
    ),
    (
        "split-spec-files.md",
        include_str!("../runbooks/split-spec-files.md"),
    ),
    (
        "manual-job-packets.md",
        include_str!("../runbooks/manual-job-packets.md"),
    ),
    (
        "baseline-drift.md",
        include_str!("../runbooks/baseline-drift.md"),
    ),
    ("respond.md", include_str!("../runbooks/respond.md")),
    (
        "persist-closeout.md",
        include_str!("../runbooks/persist-closeout.md"),
    ),
    (
        "describe-image.md",
        include_str!("../runbooks/describe-image.md"),
    ),
];

/// Bundled OKF concept files installed alongside the skill.
const BUNDLED_OKF: &[(&str, &str)] = &[
    ("index.md", include_str!("../okf/index.md")),
    ("session-cycle.md", include_str!("../okf/session-cycle.md")),
    (
        "instruction-surface.md",
        include_str!("../okf/instruction-surface.md"),
    ),
    (
        "dynamic-context.md",
        include_str!("../okf/dynamic-context.md"),
    ),
];

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_USER_PROMPT_COMMAND: &str = "agent-doc hook codex-user-prompt-submit";
const CODEX_STOP_COMMAND: &str = "agent-doc hook codex-stop";
const CODEX_MCP_SERVER_NAME: &str = "agent-doc";
const CODEX_MCP_COMMAND: &str = "agent-doc";
const CODEX_MCP_APPROVAL_MODE: &str = "approve";

fn content_for_env(env: agent_kit::detect::Environment) -> String {
    use agent_kit::detect::Environment;
    let (description, invocation_section) = match env {
        Environment::Codex => (CODEX_DESCRIPTION, CODEX_INVOCATION_SECTION),
        Environment::OpenCode => (OPENCODE_DESCRIPTION, OPENCODE_INVOCATION_SECTION),
        Environment::Generic => (GENERIC_DESCRIPTION, GENERIC_INVOCATION_SECTION),
        _ => (CLAUDE_DESCRIPTION, CLAUDE_INVOCATION_SECTION),
    };
    render_skill(env, description, invocation_section)
}

fn render_skill(
    env: agent_kit::detect::Environment,
    description: &str,
    invocation_section: &str,
) -> String {
    let rendered = replace_frontmatter_field(SKILL_TEMPLATE, "description", description)
        .expect("SKILL.md must contain a description field in frontmatter");
    let rendered = replace_frontmatter_field(&rendered, "agent-doc-version", VERSION)
        .expect("SKILL.md must contain an agent-doc-version field in frontmatter");
    let rendered = replace_markdown_section(&rendered, "## Invocation", invocation_section)
        .expect("SKILL.md must contain an ## Invocation section");
    let mut rendered = replace_auto_update_line(&rendered, auto_update_line_for_env(env))
        .expect("SKILL.md must contain the auto-update instructions");
    if is_claude_environment(env) {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(CLAUDE_QUEUE_AUTO_LOOP_SECTION.trim_start_matches('\n'));
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
    }
    rendered
}

fn is_claude_environment(env: agent_kit::detect::Environment) -> bool {
    use agent_kit::detect::Environment;
    !matches!(
        env,
        Environment::Codex | Environment::OpenCode | Environment::Generic
    )
}

fn replace_frontmatter_field(content: &str, field: &str, value: &str) -> Option<String> {
    let mut replaced = false;
    let mut in_frontmatter = false;
    let mut rendered = String::new();

    for line in content.lines() {
        if line == "---" {
            in_frontmatter = !in_frontmatter;
            rendered.push_str(line);
            rendered.push('\n');
            continue;
        }

        if in_frontmatter && line.starts_with(&format!("{field}: ")) {
            rendered.push_str(&format!("{field}: \"{value}\"\n"));
            replaced = true;
            continue;
        }

        rendered.push_str(line);
        rendered.push('\n');
    }

    if !content.ends_with('\n') {
        rendered.pop();
    }

    replaced.then_some(rendered)
}

fn replace_markdown_section(content: &str, heading: &str, replacement: &str) -> Option<String> {
    let start = content.find(heading)?;
    let after_heading = &content[start + heading.len()..];
    let next_heading = after_heading.find("\n## ");
    let end = next_heading
        .map(|idx| start + heading.len() + idx + 1)
        .unwrap_or(content.len());

    let mut rendered = String::new();
    rendered.push_str(&content[..start]);
    rendered.push_str(replacement.trim_end());
    rendered.push('\n');
    rendered.push_str(&content[end..]);
    Some(rendered)
}

fn replace_auto_update_line(content: &str, replacement: &str) -> Option<String> {
    content
        .contains(AUTO_UPDATE_LINE)
        .then(|| content.replacen(AUTO_UPDATE_LINE, replacement, 1))
}

fn auto_update_line_for_env(env: agent_kit::detect::Environment) -> &'static str {
    use agent_kit::detect::Environment;
    match env {
        Environment::Codex => CODEX_AUTO_UPDATE_LINE,
        Environment::OpenCode => OPENCODE_AUTO_UPDATE_LINE,
        Environment::Generic => AUTO_UPDATE_LINE,
        _ => CLAUDE_AUTO_UPDATE_LINE,
    }
}

fn looks_like_managed_root_agents(content: &str) -> bool {
    content.contains("user-invocable: true")
        && content.contains("agent-doc-version:")
        && content.contains("# agent-doc")
        && content.contains("## Harness Compatibility")
        && content.contains("## Workflow")
}

fn skill_rel_path_for_env(env: agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        // Codex loads `.codex/AGENTS.md` as always-on repository instructions. The
        // agent-doc workflow is trigger-scoped, so install it as a real Codex skill
        // instead of leaking it into every normal Codex task.
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/SKILL.md"),
        // A generic root `AGENTS.md` is also always-on for several harnesses. Keep
        // the generic rendering available for tests/content parity, but never
        // install it into the root instruction file.
        Environment::Generic => std::path::PathBuf::from(".agent/skills/agent-doc/SKILL.md"),
        _ => env.skill_rel_path("agent-doc"),
    }
}

fn skill_path_for_env(
    env: agent_kit::detect::Environment,
    root: Option<&Path>,
) -> std::path::PathBuf {
    let rel = skill_rel_path_for_env(env);
    match root {
        Some(root) => root.join(rel),
        None => rel,
    }
}

fn is_skill_current_for_env(
    env: agent_kit::detect::Environment,
    root: Option<&Path>,
) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    if !path.exists() {
        return Ok(false);
    }
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(existing == content_for_env(env))
}

fn install_skill_for_env(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    let content = content_for_env(env);

    if path.exists() {
        let existing = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if existing == content {
            eprintln!("[{}] skill already up to date (v{}).", env, VERSION);
            return Ok(false);
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    eprintln!(
        "[{}] installed skill v{} -> {}",
        env,
        VERSION,
        path.display()
    );
    Ok(true)
}

fn check_skill_for_env(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    if !path.exists() {
        eprintln!("Not installed. Run `agent-doc skill install` to install.");
        return Ok(false);
    }
    if is_skill_current_for_env(env, root)? {
        eprintln!("Up to date (v{}).", VERSION);
        Ok(true)
    } else {
        eprintln!(
            "Outdated. Run `agent-doc skill install` to update to v{}.",
            VERSION
        );
        Ok(false)
    }
}

fn normalized_managed_instruction_surface_for_audit(content: &str) -> String {
    strip_tsift_code_navigation_block(content)
        .trim_end()
        .to_string()
}

fn extract_tsift_code_navigation_block(content: &str) -> Option<String> {
    const START: &str = "<!-- tsift:code-navigation";
    const END: &str = "<!-- /tsift:code-navigation -->";
    let start = content.find(START)?;
    let relative_end = content[start..].find(END)?;
    let mut end = start + relative_end + END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    Some(content[start..end].trim_matches('\n').to_string())
}

fn strip_tsift_code_navigation_block(content: &str) -> String {
    const START: &str = "<!-- tsift:code-navigation";
    const END: &str = "<!-- /tsift:code-navigation -->";
    let Some(start) = content.find(START) else {
        return content.to_string();
    };
    let Some(relative_end) = content[start..].find(END) else {
        return content.to_string();
    };
    let mut end = start + relative_end + END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    let mut rendered = String::new();
    rendered.push_str(content[..start].trim_end());
    rendered.push('\n');
    rendered.push_str(content[end..].trim_start_matches('\n'));
    rendered
}

fn retire_managed_agents_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let existing =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if !looks_like_managed_root_agents(&existing) {
        return Ok(());
    }

    if let Some(tsift_block) = extract_tsift_code_navigation_block(&existing) {
        std::fs::write(path, format!("{}\n", tsift_block.trim_end()))
            .with_context(|| format!("write {}", path.display()))?;
        eprintln!(
            "[agent-doc] retired managed instruction surface, preserved tsift block -> {}",
            path.display()
        );
        return Ok(());
    }

    std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    eprintln!(
        "[agent-doc] retired managed instruction surface -> {}",
        path.display()
    );
    Ok(())
}

fn retire_managed_always_on_agents(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    retire_managed_agents_file(&base.join("AGENTS.md"))?;
    retire_managed_agents_file(&base.join(".codex/AGENTS.md"))?;
    Ok(())
}

pub(crate) fn audit_managed_instruction_surfaces(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    for path in [base.join("AGENTS.md"), base.join(".codex/AGENTS.md")] {
        if !path.exists() {
            continue;
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if looks_like_managed_root_agents(&existing) {
            anyhow::bail!(
                "retired managed agent-doc instruction surface is still present: {}. Run `agent-doc skill install --all` or reinstall the active harness to migrate it out of always-on AGENTS.md.",
                path.display()
            );
        }
    }

    for env in [
        agent_kit::detect::Environment::OpenCode,
        agent_kit::detect::Environment::Codex,
        agent_kit::detect::Environment::ClaudeCode,
    ] {
        let path = skill_path_for_env(env, Some(&base));
        if !path.exists() {
            continue;
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if !looks_like_managed_root_agents(&existing) {
            continue;
        }
        let expected = content_for_env(env);
        let normalized_existing = normalized_managed_instruction_surface_for_audit(&existing);
        let normalized_expected = normalized_managed_instruction_surface_for_audit(&expected);
        if normalized_existing != normalized_expected {
            anyhow::bail!(
                "managed agent-doc instruction surface is stale: {}. Run `agent-doc skill install --all` or reinstall the active harness before release.",
                path.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn audit_managed_okf_bundles(root: Option<&Path>) -> Result<()> {
    audit_bundled_okf_shape()?;

    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    for env in [
        agent_kit::detect::Environment::Generic,
        agent_kit::detect::Environment::OpenCode,
        agent_kit::detect::Environment::Codex,
        agent_kit::detect::Environment::Cursor,
        agent_kit::detect::Environment::ClaudeCode,
    ] {
        let instruction_path = skill_path_for_env(env, Some(&base));
        let okf_dir = base.join(okf_rel_path(&env));
        if instruction_path.exists() {
            let content = std::fs::read_to_string(&instruction_path)
                .with_context(|| format!("read {}", instruction_path.display()))?;
            audit_okf_links_for_surface(&env, &instruction_path, &content)?;
            if content.contains("okf/") && !okf_dir.is_dir() {
                anyhow::bail!(
                    "managed agent-doc OKF bundle is missing for {env}: {} references okf/ but {} does not exist. Run `agent-doc skill install --all`.",
                    instruction_path.display(),
                    okf_dir.display()
                );
            }
        }
        if okf_dir.is_dir() {
            audit_okf_dir(&env, &okf_dir)?;
        }
    }
    Ok(())
}

fn audit_bundled_okf_shape() -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    let mut concept_ids = HashSet::new();
    for (name, content) in BUNDLED_OKF {
        for link in markdown_okf_links(content, true) {
            if !canonical_names.contains(link.as_str()) {
                anyhow::bail!(
                    "bundled OKF file {name} references okf/{link}, but {link} is not bundled"
                );
            }
        }
        if content.contains("type: concept") {
            let Some(concept_id) = frontmatter_value(content, "concept_id") else {
                anyhow::bail!("bundled OKF concept {name} is missing concept_id frontmatter");
            };
            if !concept_ids.insert(concept_id.to_string()) {
                anyhow::bail!("duplicate bundled OKF concept_id `{concept_id}`");
            }
        }
    }
    Ok(())
}

fn audit_okf_links_for_surface(
    env: &agent_kit::detect::Environment,
    path: &Path,
    content: &str,
) -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    for link in markdown_okf_links(content, false) {
        if !canonical_names.contains(link.as_str()) {
            anyhow::bail!(
                "managed agent-doc instruction surface for {env} references unbundled OKF file okf/{link}: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn audit_okf_dir(env: &agent_kit::detect::Environment, okf_dir: &Path) -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    for (name, expected) in BUNDLED_OKF {
        let path = okf_dir.join(name);
        if !path.exists() {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} is missing {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if existing != *expected {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} is stale: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }

    let entries = std::fs::read_dir(okf_dir)
        .with_context(|| format!("read OKF dir {}", okf_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("read OKF dir entry under {}", okf_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect OKF entry {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !canonical_names.contains(file_name) {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} contains stale concept markdown: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }
    Ok(())
}

fn markdown_okf_links(content: &str, include_local_okf_links: bool) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = content;
    while let Some(open) = rest.find("](") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find(')') else {
            break;
        };
        let target = &rest[..close];
        if let Some(name) = okf_link_name(target, include_local_okf_links) {
            links.push(name.to_string());
        }
        rest = &rest[close + 1..];
    }
    links
}

fn okf_link_name(target: &str, include_local_okf_links: bool) -> Option<&str> {
    let target = target.split('#').next().unwrap_or(target);
    let target = target.strip_prefix("./").unwrap_or(target);
    if let Some(name) = target.strip_prefix("okf/") {
        return name.ends_with(".md").then_some(name);
    }
    if include_local_okf_links && !target.contains('/') && target.ends_with(".md") {
        return Some(target);
    }
    None
}

fn frontmatter_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        let Some((candidate, value)) = line.split_once(':') else {
            continue;
        };
        if candidate.trim() == key {
            return Some(value.trim().trim_matches('"'));
        }
    }
    None
}

fn detect_install_env() -> agent_kit::detect::Environment {
    use agent_kit::detect::Environment;

    let detected = Environment::detect();
    if !matches!(detected, Environment::Generic) {
        return detected;
    }

    if std::env::var_os("CODEX_THREAD_ID").is_some() || std::env::var_os("CODEX_CI").is_some() {
        return Environment::Codex;
    }

    detected
}

#[cfg(test)]
fn remove_markdown_section(content: &str, heading: &str) -> String {
    let Some(start) = content.find(heading) else {
        return content.to_string();
    };
    let after_heading = &content[start + heading.len()..];
    let next_heading = after_heading.find("\n## ");
    let end = next_heading
        .map(|idx| start + heading.len() + idx + 1)
        .unwrap_or(content.len());

    let mut rendered = String::new();
    rendered.push_str(&content[..start]);
    rendered.push_str(&content[end..]);
    rendered
}

/// Resolve the project root for skill installation.
///
/// When CWD is inside a git submodule (e.g., `src/agent-doc/`), the skill
/// should be installed to the superproject root, not the submodule. This
/// ensures the SKILL.md that Claude Code reads (from the project root's
/// `.claude/skills/`) matches the binary version.
fn resolve_root() -> Option<std::path::PathBuf> {
    // Try superproject first (handles submodule CWD)
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    // Fall back to git toplevel
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    None
}

/// Install bundled runbooks alongside the skill for the detected environment.
fn install_runbooks(root: Option<&Path>) -> Result<()> {
    let env = detect_install_env();
    install_runbooks_for(env, root)
}

/// Resolve the runbooks directory for a specific environment.
fn runbooks_rel_path(env: &agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        Environment::ClaudeCode => std::path::PathBuf::from(".claude/skills/agent-doc/runbooks"),
        Environment::OpenCode => std::path::PathBuf::from(".opencode/skills/agent-doc/runbooks"),
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/runbooks"),
        Environment::Cursor => std::path::PathBuf::from(".cursor/rules/runbooks"),
        Environment::Generic => std::path::PathBuf::from("runbooks"),
    }
}

/// Install bundled runbooks for a specific environment.
///
/// Reconciles the target directory against the canonical embedded set:
/// - Writes missing or out-of-date `.md` files from `BUNDLED_RUNBOOKS`.
/// - Reaps any `.md` files in the target directory that are not in the
///   canonical set, so the on-disk set converges to the embedded set across
///   upgrades (renames, deletions, splits).
///
/// Non-`.md` files are left alone — only the install-managed runbook surface
/// is reconciled.
fn install_runbooks_for(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let runbooks_dir = base.join(runbooks_rel_path(&env));
    std::fs::create_dir_all(&runbooks_dir)?;

    let canonical_names: std::collections::HashSet<&str> =
        BUNDLED_RUNBOOKS.iter().map(|(name, _)| *name).collect();

    for (name, content) in BUNDLED_RUNBOOKS {
        let path = runbooks_dir.join(name);
        let needs_write = !path.exists()
            || std::fs::read_to_string(&path)
                .map(|existing| existing != *content)
                .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, content)?;
        }
    }

    let entries = std::fs::read_dir(&runbooks_dir)
        .with_context(|| format!("read runbooks dir {}", runbooks_dir.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("read runbooks dir entry under {}", runbooks_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect runbook entry {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if canonical_names.contains(file_name) {
            continue;
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("reap stale runbook {}", path.display()))?;
        eprintln!("[{}] reaped stale runbook → {}", env, path.display());
    }

    Ok(())
}

/// Install bundled runbooks for all environments.
fn install_runbooks_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_runbooks_for(env, root)?;
    }
    Ok(())
}

/// Install bundled OKF concept files alongside the skill for the detected environment.
fn install_okf(root: Option<&Path>) -> Result<()> {
    let env = detect_install_env();
    install_okf_for(env, root)
}

/// Resolve the OKF directory for a specific environment.
fn okf_rel_path(env: &agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        Environment::ClaudeCode => std::path::PathBuf::from(".claude/skills/agent-doc/okf"),
        Environment::OpenCode => std::path::PathBuf::from(".opencode/skills/agent-doc/okf"),
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/okf"),
        Environment::Cursor => std::path::PathBuf::from(".cursor/rules/okf"),
        Environment::Generic => std::path::PathBuf::from("okf"),
    }
}

/// Install bundled OKF files for a specific environment.
///
/// The installer reconciles only Markdown files in the managed OKF directory.
/// Non-Markdown files are left alone for local notes or tool artifacts.
fn install_okf_for(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let okf_dir = base.join(okf_rel_path(&env));
    std::fs::create_dir_all(&okf_dir)?;

    let canonical_names: std::collections::HashSet<&str> =
        BUNDLED_OKF.iter().map(|(name, _)| *name).collect();

    for (name, content) in BUNDLED_OKF {
        let path = okf_dir.join(name);
        let needs_write = !path.exists()
            || std::fs::read_to_string(&path)
                .map(|existing| existing != *content)
                .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, content)?;
        }
    }

    let entries = std::fs::read_dir(&okf_dir)
        .with_context(|| format!("read OKF dir {}", okf_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("read OKF dir entry under {}", okf_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect OKF entry {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if canonical_names.contains(file_name) {
            continue;
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("reap stale OKF {}", path.display()))?;
        eprintln!("[{}] reaped stale OKF → {}", env, path.display());
    }

    Ok(())
}

/// Install bundled OKF files for all environments.
fn install_okf_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_okf_for(env, root)?;
    }
    Ok(())
}

fn install_env_artifacts(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    if matches!(env, agent_kit::detect::Environment::Codex) {
        install_codex_hook_artifacts(root)?;
    }
    if matches!(env, agent_kit::detect::Environment::OpenCode) {
        install_opencode_command_file(root)?;
    }
    Ok(())
}

fn install_env_artifacts_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_env_artifacts(env, root)?;
    }
    Ok(())
}

fn install_codex_hook_artifacts(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let codex_dir = base.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    merge_codex_hooks_json(&codex_dir.join("hooks.json"))?;
    merge_codex_config(&codex_dir.join("config.toml"), &base)?;
    Ok(())
}

const OPENCODE_COMMAND_CONTENT: &str = "\
---
description: \"Interactive document session with agent-doc\"
---

agent-doc $ARGUMENTS
";

fn install_opencode_command_file(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let commands_dir = base.join(".opencode/commands");
    std::fs::create_dir_all(&commands_dir)?;
    let path = commands_dir.join("agent-doc.md");
    let needs_write = !path.exists()
        || std::fs::read_to_string(&path)
            .map(|existing| existing != OPENCODE_COMMAND_CONTENT)
            .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, OPENCODE_COMMAND_CONTENT)
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

fn merge_codex_hooks_json(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str::<serde_json::Value>(&content)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        serde_json::json!({})
    };

    let hooks = root
        .as_object_mut()
        .context("Codex hooks.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("Codex hooks.json `hooks` must be an object")?;

    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        CODEX_USER_PROMPT_COMMAND,
        Some("Tracking active agent-doc session"),
    );
    ensure_codex_hook_command(
        hooks_map,
        "Stop",
        CODEX_STOP_COMMAND,
        Some("Checking agent-doc completion boundary"),
    );

    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// `#coinedid` mid-turn guard. `PreToolUse` fires before the tool runs, so a
/// coined `#id` is refused BEFORE it reaches source or a commit message — the
/// post-commit `session-check` warning can only report damage already done.
const COINED_ID_PRETOOLUSE_COMMAND: &str = "agent-doc hook coined-id-pre-tool-use";

/// `#preflightinbinary`. `UserPromptSubmit` fires when the `agent-doc <FILE>`
/// trigger arrives, so the binary runs its own preflight and its stdout becomes
/// the injected cycle contract — the agent gets the contract *with* the prompt
/// instead of having to remember to shell back for it. The handler recognizes
/// only a bare `agent-doc <FILE>` first line and no-ops on everything else, so
/// it cannot perturb an ordinary prompt.
const PREFLIGHT_USER_PROMPT_COMMAND: &str = "agent-doc hook preflight-user-prompt-submit";

const TURN_STATUS_ACTIVE_COMMAND: &str = "agent-doc turn-status active";
const TURN_STATUS_IDLE_COMMAND: &str = "agent-doc turn-status idle";

/// Install the Claude Code hooks that drive the turn-in-progress pane monitor
/// (`#claude-busy-status-during-active-turn`) into a `settings.json`. Idempotent
/// and additive (existing hooks, including the SessionStart autoclaim hook, are
/// preserved).
///
/// `UserPromptSubmit` → `turn-status active` (turn start); `Stop` **and**
/// `SessionStart` → `turn-status idle`. The double clear is the reliability
/// belt-and-suspenders: the Stop hook here does only trivial, idempotent,
/// best-effort work (clear a pane title) — unlike the consequential Codex closeout
/// Stop hook — so a missed Stop can only leave a stale cosmetic title, and the
/// SessionStart clear self-heals it at the next session start.
/// OpenCode plugin that drives the monitor: `chat.message` (a new user message =
/// turn start) → `turn-status active`; the `session.idle` bus event (turn end) →
/// `turn-status idle`. Best-effort — `agent-doc turn-status` no-ops outside tmux.
const OPENCODE_TURN_STATUS_PLUGIN: &str = r#"// agent-doc turn-in-progress pane monitor (#claude-busy-status-during-active-turn).
// Auto-installed by `agent-doc turn-status install`. Sets the agent's own tmux
// pane border title on turn start (chat.message) and clears it on turn end
// (session.idle). Best-effort: agent-doc turn-status no-ops outside tmux.
export const AgentDocTurnStatus = async ({ $ }) => ({
  "chat.message": async () => {
    try { await $`agent-doc turn-status active` } catch {}
  },
  event: async ({ event }) => {
    if (event.type === "session.idle") {
      try { await $`agent-doc turn-status idle` } catch {}
    }
  },
})
"#;

/// Install the turn-in-progress pane monitor for all three harnesses
/// (#claude-busy-status-during-active-turn). Idempotent + additive per harness.
/// `base`, when set, roots every harness path under it (project-relative; used by
/// tests and `--dir`). Otherwise `user` selects each harness's user config dir,
/// else the project-local (cwd) paths.
pub fn install_turn_status_hooks(base: Option<&Path>, user: bool, tmux: bool) -> Result<()> {
    // Claude — settings.json: UserPromptSubmit/Stop/SessionStart.
    let claude = turn_status_path(base, user, ".claude/settings.json", ".claude/settings.json");
    ensure_parent_dir(&claude)?;
    merge_claude_turn_status_hooks(&claude)?;
    println!("[turn-status] claude   -> {}", claude.display());

    // Codex — hooks.json: UserPromptSubmit/Stop (Codex has no SessionStart event).
    let codex = turn_status_path(base, user, ".codex/hooks.json", ".codex/hooks.json");
    ensure_parent_dir(&codex)?;
    merge_codex_turn_status_hooks(&codex)?;
    println!("[turn-status] codex    -> {}", codex.display());

    // OpenCode — plugin file: chat.message + session.idle (no JSON hook events).
    let opencode = turn_status_path(
        base,
        user,
        ".opencode/plugin/agent-doc-turn-status.js",
        ".config/opencode/plugin/agent-doc-turn-status.js",
    );
    ensure_parent_dir(&opencode)?;
    std::fs::write(&opencode, OPENCODE_TURN_STATUS_PLUGIN)
        .with_context(|| format!("write {}", opencode.display()))?;
    println!("[turn-status] opencode -> {}", opencode.display());

    println!("  start (UserPromptSubmit / chat.message)   -> {TURN_STATUS_ACTIVE_COMMAND}");
    println!("  end   (Stop / SessionStart / session.idle) -> {TURN_STATUS_IDLE_COMMAND}");

    // Visibility: the pane-border title is only shown when tmux `pane-border-status`
    // is enabled. `--tmux` applies it to the running server immediately (safe,
    // non-destructive); persisting it stays a recommendation because tmux config
    // paths are user-specific (symlinks / dotfiles repos) and risky to auto-edit.
    if tmux {
        let applied = std::process::Command::new("tmux")
            .args(["set", "-g", "pane-border-status", "top"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if applied {
            println!("  tmux: applied `pane-border-status top` to the running server");
        } else {
            println!(
                "  tmux: could not apply `pane-border-status` (no running server / not in tmux)"
            );
        }
        println!("  tmux: to persist, add to your tmux config: set -g pane-border-status top");
    } else {
        println!(
            "  visibility: enable the pane border to see it — `agent-doc turn-status install --tmux` (applies now), or add `set -g pane-border-status top` to your tmux config"
        );
    }
    Ok(())
}

fn turn_status_path(
    base: Option<&Path>,
    user: bool,
    project_rel: &str,
    user_rel: &str,
) -> std::path::PathBuf {
    if let Some(b) = base {
        return b.join(project_rel);
    }
    if user && let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home).join(user_rel);
    }
    std::path::PathBuf::from(project_rel)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    Ok(())
}

/// Merge the turn-status hooks into a Codex `hooks.json`. Same shape as the
/// Claude settings.json; Codex has no `SessionStart` event so only the start/end
/// pair is wired (a missed Stop self-heals when the next `UserPromptSubmit`
/// re-asserts the active title).
fn merge_codex_turn_status_hooks(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<serde_json::Value>(&content)
                .with_context(|| format!("parse {}", path.display()))?
        }
    } else {
        serde_json::json!({})
    };
    let hooks = root
        .as_object_mut()
        .context("hooks.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("hooks.json `hooks` must be an object")?;
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        TURN_STATUS_ACTIVE_COMMAND,
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None);
    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Merge the turn-status hooks into a Claude `settings.json`. Claude's hook shape
/// matches Codex `hooks.json`, so this reuses the same idempotent merge helper.
fn merge_claude_turn_status_hooks(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<serde_json::Value>(&content)
                .with_context(|| format!("parse {}", path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let hooks = root
        .as_object_mut()
        .context("settings.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("settings.json `hooks` must be an object")?;

    // Shared shape with Codex hooks.json — `ensure_codex_hook_command` is
    // harness-agnostic (a command-hook merger), reused here for Claude.
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        TURN_STATUS_ACTIVE_COMMAND,
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None);
    ensure_codex_hook_command(hooks_map, "SessionStart", TURN_STATUS_IDLE_COMMAND, None);
    // No matcher: the handler filters by tool name itself and allows everything
    // it does not recognize, so the guard cannot wedge a turn on an unknown tool.
    ensure_codex_hook_command(hooks_map, "PreToolUse", COINED_ID_PRETOOLUSE_COMMAND, None);
    // `#preflightinbinary`: run preflight in-binary on the trigger prompt so the
    // cycle contract arrives with the prompt rather than a round trip later.
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        PREFLIGHT_USER_PROMPT_COMMAND,
        None,
    );

    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn ensure_codex_hook_command(
    hooks_map: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
    status_message: Option<&str>,
) {
    let entries = hooks_map
        .entry(event.to_string())
        .or_insert_with(|| serde_json::json!([]));
    let Some(entry_array) = entries.as_array_mut() else {
        *entries = serde_json::json!([]);
        return ensure_codex_hook_command(hooks_map, event, command, status_message);
    };

    let already_present = entry_array.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("type").and_then(|v| v.as_str()) == Some("command")
                        && hook.get("command").and_then(|v| v.as_str()) == Some(command)
                })
            })
            .unwrap_or(false)
    });
    if already_present {
        return;
    }

    let mut hook = serde_json::Map::new();
    hook.insert("type".to_string(), serde_json::json!("command"));
    hook.insert("command".to_string(), serde_json::json!(command));
    if let Some(status) = status_message {
        hook.insert("statusMessage".to_string(), serde_json::json!(status));
    }
    entry_array.push(serde_json::json!({ "hooks": [hook] }));
}

fn merge_codex_config(path: &Path, project_root: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        toml::from_str::<toml::Value>(&content)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let project_root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.into());

    if !root.is_table() {
        anyhow::bail!("Codex config at {} must be a TOML table", path.display());
    }

    let root_table = root.as_table_mut().expect("checked table");
    let features = root_table
        .entry("features".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let features_table = features
        .as_table_mut()
        .context("Codex config `features` must be a table")?;
    features_table.remove("codex_hooks");
    features_table.insert("hooks".to_string(), toml::Value::Boolean(true));

    let mcp_servers = root_table
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let mcp_servers_table = mcp_servers
        .as_table_mut()
        .context("Codex config `mcp_servers` must be a table")?;
    let mut server = toml::map::Map::new();
    server.insert(
        "command".to_string(),
        toml::Value::String(CODEX_MCP_COMMAND.to_string()),
    );
    server.insert(
        "default_tools_approval_mode".to_string(),
        toml::Value::String(CODEX_MCP_APPROVAL_MODE.to_string()),
    );
    server.insert(
        "args".to_string(),
        toml::Value::Array(vec![
            toml::Value::String("mcp".to_string()),
            toml::Value::String("serve".to_string()),
            toml::Value::String("--project-root".to_string()),
            toml::Value::String(project_root.display().to_string()),
        ]),
    );
    mcp_servers_table.insert(
        CODEX_MCP_SERVER_NAME.to_string(),
        toml::Value::Table(server),
    );

    std::fs::write(path, toml::to_string_pretty(&root)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
#[allow(dead_code)]
pub fn install_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let env = detect_install_env();
    install_skill_for_env(env, resolved.as_deref())?;
    install_runbooks(resolved.as_deref())?;
    install_okf(resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    retire_managed_always_on_agents(resolved.as_deref())
}

/// Public entry point (resolves to superproject root, called from main).
#[allow(dead_code)]
pub fn install() -> Result<()> {
    install_at(None)
}

/// Install and return whether the file was actually updated (not just already up to date).
pub fn install_and_check_updated() -> Result<bool> {
    let env = detect_install_env();
    let resolved = resolve_root();

    // Check if already up to date before install
    let was_current = is_skill_current_for_env(env, resolved.as_deref())?;

    install_skill_for_env(env, resolved.as_deref())?;
    install_runbooks(resolved.as_deref())?;
    install_okf(resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    retire_managed_always_on_agents(resolved.as_deref())?;
    Ok(!was_current)
}

/// Install the skill for a specific harness environment.
pub fn install_for(env: agent_kit::detect::Environment) -> Result<()> {
    let resolved = resolve_root();
    install_skill_for_env(env, resolved.as_deref())?;
    install_runbooks_for(env, resolved.as_deref())?;
    install_okf_for(env, resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    retire_managed_always_on_agents(resolved.as_deref())
}

/// Install the skill for all supported harnesses.
pub fn install_all() -> Result<()> {
    let resolved = resolve_root();
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_skill_for_env(env, resolved.as_deref())?;
    }
    install_runbooks_all(resolved.as_deref())?;
    install_okf_all(resolved.as_deref())?;
    install_env_artifacts_all(resolved.as_deref())?;
    retire_managed_always_on_agents(resolved.as_deref())
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let up_to_date = check_skill_for_env(detect_install_env(), resolved.as_deref())?;
    if !up_to_date {
        std::process::exit(1);
    }
    Ok(())
}

/// Public entry point (resolves to superproject root, called from main).
pub fn check() -> Result<()> {
    check_at(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_kit::detect::Environment;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => {
                    // SAFETY: test-only process-local env mutation, restored to prior value.
                    unsafe { std::env::set_var(self.key, value) };
                }
                None => {
                    // SAFETY: test-only process-local env mutation, restored to prior absence.
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
    }

    #[test]
    fn bundled_skill_is_not_empty() {
        assert!(!SKILL_TEMPLATE.is_empty());
    }

    #[test]
    fn bundled_skill_contains_agent_doc() {
        assert!(SKILL_TEMPLATE.contains("agent-doc"));
    }

    #[test]
    fn bundled_skill_hot_path_stays_compact() {
        assert!(
            line_count(SKILL_TEMPLATE) <= 140,
            "SKILL.md hot path grew to {} lines",
            line_count(SKILL_TEMPLATE)
        );
    }

    #[test]
    fn rendered_claude_skill_includes_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::ClaudeCode);
        assert!(
            rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Claude-rendered SKILL.md must include the auto-loop section header"
        );
        assert!(
            rendered.contains("Skill") && rendered.contains("skill: \"loop\""),
            "auto-loop section must instruct invoking the Skill tool with skill: \"loop\""
        );
        assert!(
            rendered.contains("AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP"),
            "auto-loop section must name the env hard-cap"
        );
        assert!(
            rendered.contains("queue_active") && rendered.contains("queue_prompts"),
            "auto-loop section must reference preflight queue fields"
        );
        assert!(
            rendered.contains("\"persisted\""),
            "auto-loop section must make persisted-active queues continuation-eligible (#active-queue-persisted-no-continue)"
        );
        // #degraded-ipc-no-stall: a proven committed transition continues the
        // queue, while a missing Lazily visibility proof leaves the captured
        // intent retained in the sole state ledger.
        assert!(
            rendered.contains("#degraded-ipc-no-stall"),
            "auto-loop section must carry the degraded-IPC no-stall rule"
        );
        assert!(
            rendered.contains("monotonic write pipeline")
                && rendered.contains("missing Lazily visibility proof")
                && rendered.contains("state.db")
                && rendered.contains("PID-scoped endpoint")
                && rendered.contains("No filesystem delivery or reload broadcast"),
            "auto-loop section must distinguish a proven ledger closeout from retained Lazily delivery"
        );
        assert!(
            rendered.contains("queue_continuation_guidance"),
            "auto-loop section must point at the binary-authoritative queue_continuation_guidance field"
        );
    }

    #[test]
    fn rendered_codex_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Codex);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Codex-rendered SKILL.md must NOT include the Claude-only auto-loop section (Codex uses its own Stop hook)"
        );
    }

    #[test]
    fn rendered_opencode_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::OpenCode);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "OpenCode-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_generic_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Generic);
        assert!(
            !rendered.contains("## Auto-loop while queue is active (Claude Code)"),
            "Generic-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_harness_content_stays_compact() {
        for env in [
            Environment::ClaudeCode,
            Environment::OpenCode,
            Environment::Codex,
            Environment::Generic,
        ] {
            let content = super::content_for_env(env);
            // 152: raised from 150 by `#preflightinbinary`, which adds one
            // hot-path digest bullet. The bullet is what flips the default —
            // an agent that does not read it re-runs preflight every turn — so
            // it earns a line. Keep the headroom small: the guard exists to
            // catch drift, and a generous ceiling stops catching it.
            assert!(
                line_count(&content) <= 152,
                "{env:?} rendered instruction surface grew to {} lines",
                line_count(&content)
            );
        }
    }

    #[test]
    fn bundled_skill_template_contains_auto_update_line() {
        assert!(SKILL_TEMPLATE.contains(AUTO_UPDATE_LINE));
    }

    #[test]
    fn detect_install_env_treats_codex_thread_id_as_codex() {
        let _env_lock = crate::test_support::env_lock();
        let _claude = EnvVarGuard::unset("CLAUDE_CODE");
        let _claude_ep = EnvVarGuard::unset("CLAUDE_CODE_ENTRYPOINT");
        let _opencode = EnvVarGuard::unset("OPENCODE");
        let _cursor = EnvVarGuard::unset("CURSOR_SESSION_ID");
        let _cursor2 = EnvVarGuard::unset("CURSOR");
        let _code = EnvVarGuard::unset("CODEX");
        let _code_cli = EnvVarGuard::unset("CODEX_CLI");
        let _thread = EnvVarGuard::set("CODEX_THREAD_ID", "thread-123");
        let _ci = EnvVarGuard::unset("CODEX_CI");

        assert_eq!(super::detect_install_env(), Environment::Codex);
    }

    /// Resolve expected skill path using the explicit test environment.
    fn expected_path(dir: &std::path::Path) -> std::path::PathBuf {
        super::skill_path_for_env(Environment::ClaudeCode, Some(dir))
    }

    fn install_test(root: Option<&std::path::Path>) -> anyhow::Result<()> {
        super::install_skill_for_env(Environment::ClaudeCode, root).map(|_| ())
    }

    fn line_count(content: &str) -> usize {
        content.lines().count()
    }

    fn assert_codex_mcp_config(config: &toml::Value, root: &std::path::Path) {
        let server = &config["mcp_servers"][CODEX_MCP_SERVER_NAME];
        assert_eq!(server["command"].as_str(), Some(CODEX_MCP_COMMAND));
        assert_eq!(
            server["default_tools_approval_mode"].as_str(),
            Some(CODEX_MCP_APPROVAL_MODE)
        );
        let args: Vec<&str> = server["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let canonical_root = std::fs::canonicalize(root).unwrap();
        assert_eq!(
            args,
            vec![
                "mcp",
                "serve",
                "--project-root",
                canonical_root.to_str().unwrap()
            ]
        );
    }

    #[test]
    fn install_creates_file() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        assert!(path.exists(), "skill not found at {}", path.display());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        assert!(!path.exists());
    }

    #[test]
    fn install_creates_runbooks_claude() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbook_path = dir
            .path()
            .join(".claude/skills/agent-doc/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("Compact Exchange"));
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_creates_runbooks_codex() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let runbook_path = dir
            .path()
            .join(".codex/skills/agent-doc/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "codex runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_runbooks_reaps_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        std::fs::create_dir_all(&runbooks_dir).unwrap();

        let sentinel = runbooks_dir.join("sentinel-stale.md");
        std::fs::write(&sentinel, "# stale runbook removed in a later release\n").unwrap();
        let plugin_install = runbooks_dir.join("plugin-install.md");
        std::fs::write(&plugin_install, "# stale runbook\n").unwrap();
        let kept_non_md = runbooks_dir.join("README.txt");
        std::fs::write(&kept_non_md, "non-md file should survive\n").unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        assert!(
            !sentinel.exists(),
            "sentinel stale runbook should be reaped: {}",
            sentinel.display()
        );
        assert!(
            !plugin_install.exists(),
            "stale plugin-install.md should be reaped"
        );
        assert!(
            kept_non_md.exists(),
            "non-md files in runbooks dir must not be reaped"
        );

        let installed: std::collections::HashSet<String> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
            .collect();
        let canonical: std::collections::HashSet<String> = super::BUNDLED_RUNBOOKS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(
            installed, canonical,
            "post-install runbook set must match canonical embedded set"
        );
    }

    #[test]
    fn install_runbooks_is_no_op_on_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        let first: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let mtime = entry.metadata().ok()?.modified().ok()?;
                let name = entry.file_name().to_str()?.to_string();
                Some((name, mtime))
            })
            .collect();

        std::thread::sleep(std::time::Duration::from_millis(20));
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        for (name, mtime_before) in &first {
            let path = runbooks_dir.join(name);
            let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert_eq!(
                *mtime_before, mtime_after,
                "second install must not rewrite unchanged runbook {}",
                name
            );
        }
    }

    #[test]
    fn install_runbooks_reaps_for_codex_and_opencode() {
        let dir = tempfile::tempdir().unwrap();
        for (env, rel) in [
            (Environment::Codex, ".codex/skills/agent-doc/runbooks"),
            (Environment::OpenCode, ".opencode/skills/agent-doc/runbooks"),
        ] {
            let runbooks_dir = dir.path().join(rel);
            std::fs::create_dir_all(&runbooks_dir).unwrap();
            let sentinel = runbooks_dir.join("legacy-runbook.md");
            std::fs::write(&sentinel, "# legacy\n").unwrap();

            super::install_runbooks_for(env, Some(dir.path())).unwrap();

            assert!(
                !sentinel.exists(),
                "stale runbook under {} should be reaped",
                rel
            );
            assert!(runbooks_dir.join("commit.md").exists());
        }
    }

    #[test]
    fn install_okf_creates_concepts_for_each_harness() {
        let dir = tempfile::tempdir().unwrap();
        for (env, rel) in [
            (Environment::ClaudeCode, ".claude/skills/agent-doc/okf"),
            (Environment::Codex, ".codex/skills/agent-doc/okf"),
            (Environment::OpenCode, ".opencode/skills/agent-doc/okf"),
            (Environment::Cursor, ".cursor/rules/okf"),
            (Environment::Generic, "okf"),
        ] {
            super::install_okf_for(env, Some(dir.path())).unwrap();

            let index = dir.path().join(rel).join("index.md");
            assert!(index.exists(), "missing OKF index at {}", index.display());
            let content = std::fs::read_to_string(&index).unwrap();
            assert!(content.contains("Agent Doc OKF Index"));
            assert!(content.contains("session-cycle.md"));
        }
    }

    #[test]
    fn install_okf_reaps_stale_markdown_but_keeps_local_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let okf_dir = dir.path().join(".claude/skills/agent-doc/okf");
        std::fs::create_dir_all(&okf_dir).unwrap();
        let stale = okf_dir.join("legacy-concept.md");
        std::fs::write(&stale, "---\ntype: concept\n---\n# Legacy\n").unwrap();
        let kept = okf_dir.join("notes.txt");
        std::fs::write(&kept, "local note\n").unwrap();

        super::install_okf_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        assert!(!stale.exists(), "stale OKF markdown should be reaped");
        assert!(kept.exists(), "non-md OKF artifacts should be preserved");
        assert!(okf_dir.join("instruction-surface.md").exists());
    }

    #[test]
    fn audit_managed_okf_bundles_accepts_current_install() {
        let dir = tempfile::tempdir().unwrap();
        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();

        super::audit_managed_okf_bundles(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_missing_concept() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::remove_file(
            dir.path()
                .join(".codex/skills/agent-doc/okf/session-cycle.md"),
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert!(err.to_string().contains("session-cycle.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_stale_concept() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path()
                .join(".codex/skills/agent-doc/okf/dynamic-context.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale"));
        assert!(err.to_string().contains("dynamic-context.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_extra_managed_markdown() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path().join(".codex/skills/agent-doc/okf/legacy.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale concept markdown"));
        assert!(err.to_string().contains("legacy.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_unbundled_surface_link() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex/skills/agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".codex/skills/agent-doc/SKILL.md"),
            "# agent-doc\n\nSee [missing](okf/missing.md).\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("unbundled OKF file"));
        assert!(err.to_string().contains("missing.md"));
    }

    #[test]
    fn installed_harness_runbooks_include_commit_invariant() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains(
                "Every appended `agent-doc` session response is one complete, validated write+commit"
            ));
            assert!(content.contains("agent-doc respond <FILE>"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
    }

    #[test]
    fn installed_harness_pending_ops_runbooks_cover_plan_backed_items() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("create the plan file"));
            assert!(content.contains("include that exact plan"));
            assert!(content.contains("file path in the item text"));
            assert!(content.contains("plan-spec2-rollout.md"));
            assert!(content.contains("one flush-left backlog item per"));
            assert!(content.contains("queue entries and closeouts should target"));
        }
    }

    #[test]
    fn installed_harness_runbooks_share_manual_repair_rule() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("## Harness-Native Entrypoints"));
            assert!(content.contains("executable workflow entry"));
            assert!(content.contains(
                "Do **not** end a normal harness-native `agent-doc` turn with \"not committed\""
            ));
            assert!(content.contains("## Manual Repair Default"));
            assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
        assert!(codex.contains("agent-doc session-check <FILE>"));
        assert!(codex.contains("Do **not** report success or stop"));
        assert!(opencode.contains("## OpenCode"));
        assert!(opencode.contains("Write-back"));
    }

    #[test]
    fn install_for_codex_writes_codex_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();

        let path = dir.path().join(".codex/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(
            content.contains("agent-doc skill install --harness codex` without a reload request")
        );
        assert!(
            content.contains("re-read the installed `.codex/skills/agent-doc/SKILL.md` completely")
        );
        assert!(content.contains("Do not call `agent-doc session restart-supervisor`"));
        assert!(!content.contains("codex resume --last"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
        assert!(
            !dir.path().join(".codex/AGENTS.md").exists(),
            "Codex install must not write always-on .codex/AGENTS.md"
        );
    }

    #[test]
    fn install_for_opencode_writes_opencode_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::OpenCode, Some(dir.path())).unwrap();

        let path = dir.path().join(".opencode/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("installed OpenCode skill"));
        assert!(content.contains("agent-doc respond <FILE>"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn install_for_codex_retires_managed_always_on_agents_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        stale_root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex/AGENTS.md"),
            super::content_for_env(Environment::Codex),
        )
        .unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(root.contains("<!-- tsift:code-navigation v=0.1.42 -->"));
        assert!(!root.contains("# agent-doc"));
        assert!(!dir.path().join(".codex/AGENTS.md").exists());
        let codex_skill =
            std::fs::read_to_string(dir.path().join(".codex/skills/agent-doc/SKILL.md")).unwrap();
        assert!(
            codex_skill
                .contains("agent-doc skill install --harness codex` without a reload request")
        );
    }

    #[test]
    fn install_for_codex_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        let custom = "# Custom Project Instructions\n\nKeep this file untouched.\n";
        std::fs::write(dir.path().join("AGENTS.md"), custom).unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert_eq!(root, custom);
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_root_agents_surface() {
        let dir = tempfile::tempdir().unwrap();
        let stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains("AGENTS.md"));
        assert!(message.contains("agent-doc skill install --all"));
    }

    #[test]
    fn retire_managed_root_agents_preserves_tsift_navigation_block() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic);
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(root.contains("<!-- tsift:code-navigation v=0.1.42 -->"));
        assert!(root.contains("Run `tsift status`."));
        assert!(!root.contains("# agent-doc"));
        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_root_agents_even_with_tsift() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains("AGENTS.md"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_codex_agents() {
        let dir = tempfile::tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let stale_codex = super::content_for_env(Environment::Codex).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(codex_dir.join("AGENTS.md"), stale_codex).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains(".codex"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# Custom Project Instructions\n\nKeep this file untouched.\n",
        )
        .unwrap();

        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn install_for_codex_writes_hooks_json_and_feature_flag() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks_path = dir.path().join(".codex/hooks.json");
        let config_path = dir.path().join(".codex/config.toml");
        assert!(hooks_path.exists(), "missing {}", hooks_path.display());
        assert!(config_path.exists(), "missing {}", config_path.display());

        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let stop_hooks = hooks["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(CODEX_STOP_COMMAND)
                && hook["type"].as_str() == Some("command")
        }));
        let submit_hooks = hooks["hooks"]["UserPromptSubmit"][0]["hooks"]
            .as_array()
            .unwrap();
        assert!(
            submit_hooks
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_USER_PROMPT_COMMAND))
        );

        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
        assert!(
            dir.path().join(".codex/skills/agent-doc/SKILL.md").exists(),
            "Codex workflow should be installed as a skill, not .codex/AGENTS.md"
        );
        assert!(!dir.path().join(".codex/AGENTS.md").exists());
    }

    #[test]
    fn install_turn_status_hooks_all_harnesses_idempotent_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        // A pre-existing unrelated Claude hook (autoclaim SessionStart) must survive.
        let claude_settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        std::fs::write(
            &claude_settings,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"agent-doc autoclaim"}]}]}}"#,
        )
        .unwrap();

        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap();
        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap(); // idempotent re-run

        let cmds = |v: &serde_json::Value, event: &str| -> Vec<String> {
            v["hooks"][event]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default())
                        .filter_map(|h| h["command"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        // Claude: start + (Stop, SessionStart) end hooks; autoclaim preserved; idempotent.
        let claude: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
        assert!(
            cmds(&claude, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string())
        );
        assert!(cmds(&claude, "Stop").contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        let ss = cmds(&claude, "SessionStart");
        assert!(
            ss.contains(&"agent-doc autoclaim".to_string()),
            "autoclaim preserved: {ss:?}"
        );
        assert!(ss.contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        assert_eq!(
            ss.iter()
                .filter(|c| c.as_str() == TURN_STATUS_IDLE_COMMAND)
                .count(),
            1,
            "idempotent: {ss:?}"
        );

        // Codex: start + Stop end (no SessionStart event for Codex).
        let codex: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        assert!(cmds(&codex, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string()));
        assert!(cmds(&codex, "Stop").contains(&TURN_STATUS_IDLE_COMMAND.to_string()));

        // OpenCode: plugin file wired to both turn-status commands via chat.message + session.idle.
        let oc =
            std::fs::read_to_string(dir.path().join(".opencode/plugin/agent-doc-turn-status.js"))
                .unwrap();
        assert!(oc.contains("turn-status active"), "{oc}");
        assert!(oc.contains("session.idle"), "{oc}");
        assert!(oc.contains("turn-status idle"), "{oc}");
    }

    #[test]
    fn install_for_codex_preserves_existing_hook_and_config_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "Stop": [
                        {
                            "hooks": [
                                { "type": "command", "command": "echo existing-stop" }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".codex/config.toml"),
            "[sandbox]\ndefault = \"workspace-write\"\n",
        )
        .unwrap();

        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        let stop_hooks = hooks["hooks"]["Stop"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some("echo existing-stop"))
        }));
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_STOP_COMMAND))
        }));

        let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["sandbox"]["default"].as_str(),
            Some("workspace-write")
        );
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
    }

    #[test]
    fn install_runbooks_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_all(Some(dir.path())).unwrap();

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".cursor/rules/runbooks/compact-exchange.md")
                .exists()
        );
    }

    #[test]
    fn install_okf_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_okf_all(Some(dir.path())).unwrap();

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".opencode/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(dir.path().join(".cursor/rules/okf/index.md").exists());
    }

    #[test]
    fn bundled_skill_contains_harness_preamble() {
        assert!(SKILL_TEMPLATE.contains("Harness Compatibility"));
        assert!(SKILL_TEMPLATE.contains("harness-invocation.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/commit.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/command-synonyms.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/compound-task-steering.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/planning-dispatch.md"));
        assert!(SKILL_TEMPLATE.contains("okf/index.md"));
    }

    #[test]
    fn bundled_skill_contains_backlog_capture_rules() {
        assert!(SKILL_TEMPLATE.contains("Backlog capture rule"));
        assert!(SKILL_TEMPLATE.contains("[recommended]"));
        assert!(SKILL_TEMPLATE.contains("beginning of `agent:backlog`"));
        assert!(SKILL_TEMPLATE.contains("adjacent to its predecessor"));
        assert!(SKILL_TEMPLATE.contains("multi-phase implementation work"));
        assert!(SKILL_TEMPLATE.contains("prefer one backlog ID per actionable phase"));
        assert!(SKILL_TEMPLATE.contains("`do #id` closeout rule"));
        assert!(SKILL_TEMPLATE.contains("--done <id>"));
        assert!(SKILL_TEMPLATE.contains("pending_done_guard"));
    }

    #[test]
    fn bundled_skill_treats_imperative_document_edits_as_executable_work() {
        assert!(SKILL_TEMPLATE.contains("Imperative edits are executable directives"));
        assert!(
            SKILL_TEMPLATE.contains("Do not require the same instruction to be repeated in chat")
        );
        assert!(
            SKILL_TEMPLATE
                .contains("MCP auth / OAuth steps are sub-steps, not closeout boundaries")
        );
        assert!(SKILL_TEMPLATE.contains(
            "Do not keep appending \"starting/continuing\" status prose while the requested work remains undone"
        ));
    }

    #[test]
    fn bundled_skill_preserves_operator_visible_document_authority() {
        assert!(SKILL_TEMPLATE.contains("Operator-visible document text is authoritative"));
        assert!(SKILL_TEMPLATE.contains("content_ours"));
        assert!(SKILL_TEMPLATE.contains(
            "Snapshots and incomplete token captures are backup/audit state, not hot-path authority"
        ));
        assert!(SKILL_TEMPLATE.contains("fail closed or retry through the editor"));
    }

    #[test]
    fn bundled_skill_treats_harness_native_entrypoints_as_binary_owned_cycles() {
        assert!(SKILL_TEMPLATE.contains(
            "Harness-native `agent-doc` entrypoints start the binary-owned response cycle"
        ));
        assert!(SKILL_TEMPLATE.contains("executable workflow start"));
        assert!(SKILL_TEMPLATE.contains("generic document-editing request"));
        assert!(
            SKILL_TEMPLATE
                .contains("Do not manually patch the final assistant response into the document")
        );
        assert!(SKILL_TEMPLATE.contains("agent-doc respond <FILE>"));
        assert!(SKILL_TEMPLATE.contains("binary resolves and commits the turn"));
        assert!(
            SKILL_TEMPLATE
                .contains("stage and commit only the intended non-session repo files first")
        );
        assert!(SKILL_TEMPLATE.contains("code-enforced-directives.md"));
    }

    #[test]
    fn bundled_skill_contains_manual_repair_write_commit_rule() {
        assert!(SKILL_TEMPLATE.contains("Manual repair / missed patchback rule (all harnesses)"));
        assert!(
            SKILL_TEMPLATE
                .contains("do **not** patch the assistant response directly into the file")
        );
        assert!(SKILL_TEMPLATE.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(SKILL_TEMPLATE.contains("bare `agent-doc write`"));
    }

    #[test]
    fn bundled_skill_delegates_captured_finalize_recovery_to_the_binary() {
        assert!(SKILL_TEMPLATE.contains("Captured finalize is binary-owned (all harnesses)"));
        assert!(SKILL_TEMPLATE.contains("owned by the keyed supervisor worker"));
        assert!(SKILL_TEMPLATE.contains("Do not recapture, re-answer"));
        assert!(SKILL_TEMPLATE.contains("bounded retry and exact-once commit semantics"));
        assert!(SKILL_TEMPLATE.contains("exact editor save-flush"));
        assert!(!SKILL_TEMPLATE.contains("then retry the same closeout once"));
    }

    #[test]
    fn development_claude_skill_version_matches_the_bundle() {
        let development_skill = include_str!("../.claude/skills/agent-doc/SKILL.md");
        let expected = format!("agent-doc-version: \"{VERSION}\"");
        assert!(SKILL_TEMPLATE.contains(&expected));
        assert!(
            development_skill.contains(&expected),
            "development Claude skill must track the bundled release version {VERSION}"
        );
    }

    #[test]
    fn bundled_skill_contains_binary_owned_respond_commit_invariant() {
        assert!(SKILL_TEMPLATE.contains("agent-doc respond <FILE>"));
        assert!(
            SKILL_TEMPLATE
                .contains("unless the user explicitly told you to leave the response uncommitted")
        );
        assert!(SKILL_TEMPLATE.contains("requires the cycle to reach `committed`"));
        assert!(SKILL_TEMPLATE.contains("agent-doc session-check <FILE>"));
        assert!(SKILL_TEMPLATE.contains("final document-mutation boundary for the cycle"));
        assert!(SKILL_TEMPLATE.contains(
            "After `respond` / `write --commit`, do not start more long-running task work"
        ));
        assert!(SKILL_TEMPLATE.contains("`finalize` is its compatibility alias"));
    }

    #[test]
    fn bundled_skill_compact_entry_uses_commit_closeout() {
        assert!(SKILL_TEMPLATE.contains("agent-doc compact <FILE> --commit"));
        assert!(SKILL_TEMPLATE.contains("compact exchange <FILE>"));
    }

    #[test]
    fn bundled_skill_contains_model_short_name_attribution_rule() {
        assert!(SKILL_TEMPLATE.contains("### Re: topic — gpt-5"));
        assert!(SKILL_TEMPLATE.contains("### Re: topic — opus-4-6"));
        assert!(SKILL_TEMPLATE.contains("Never use the harness label (`codex`, `claude`)"));
    }

    #[test]
    fn bundled_skill_requires_oldest_first_exchange_tail_reconciliation() {
        assert!(SKILL_TEMPLATE.contains("Do not stop at the newest question"));
        assert!(SKILL_TEMPLATE.contains("each unresolved prompt in that tail"));
    }

    #[test]
    fn codex_content_uses_plain_text_invocation() {
        let content = super::content_for_env(Environment::Codex);

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(
            content.contains("agent-doc skill install --harness codex` without a reload request")
        );
        assert!(
            content.contains("re-read the installed `.codex/skills/agent-doc/SKILL.md` completely")
        );
        assert!(content.contains("Do not call `agent-doc session restart-supervisor`"));
        assert!(!content.contains("codex resume --last"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("generic document-editing request"));
        assert!(content.contains("final document-mutation boundary for the cycle"));
        assert!(content.contains("do not start more long-running task work for that same turn"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains(".codex/config.toml"));
        assert!(content.contains("fail-closed backstop"));
        assert!(content.contains("MCP auth / OAuth steps are sub-steps"));
        assert!(content.contains("Project-scoped remote hosts"));
        assert!(content.contains("globally approved SSH commands"));
        assert!(content.contains("project-local `.agent-doc/config.toml`"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
        assert!(content.contains("Imperative edits are executable directives"));
        assert!(content.contains("Do not require the same instruction to be repeated in chat"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn opencode_content_uses_slash_command_invocation() {
        let content = super::content_for_env(Environment::OpenCode);

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("installed OpenCode skill"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("final document-mutation boundary for the cycle"));
        assert!(content.contains("MCP auth / OAuth steps are sub-steps"));
        assert!(content.contains("Imperative edits are executable directives"));
        assert!(content.contains("Do not require the same instruction to be repeated in chat"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
        assert!(!content.contains("Codex CLI will reject it"));
        assert!(!content.contains("In OpenCode, invoke agent-doc by writing"));
    }

    #[test]
    fn install_for_opencode_creates_command_file() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();

        let path = dir.path().join(".opencode/commands/agent-doc.md");
        assert!(path.exists(), "command file should exist at {path:?}");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("agent-doc $ARGUMENTS"));
        assert!(content.contains("description:"));
    }

    #[test]
    fn install_for_opencode_command_file_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let first =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let second =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn install_env_artifacts_creates_opencode_command() {
        let dir = tempfile::tempdir().unwrap();

        super::install_env_artifacts(Environment::OpenCode, Some(dir.path())).unwrap();

        assert!(dir.path().join(".opencode/commands/agent-doc.md").exists());
    }

    #[test]
    fn claude_content_keeps_slash_invocation() {
        let content = super::content_for_env(Environment::ClaudeCode);

        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("TRIGGER: user invokes /agent-doc <file>"));
        assert!(content.contains("agent-doc skill install --harness claude --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains("agent_doc_auto_compact"));
        assert!(content.contains("Use `--reload compact`"));
        assert!(content.contains("stale instruction drift"));
        assert!(content.contains("continue this turn"));
        assert!(content.contains("binary-owned response cycle"));
        assert!(content.contains("Do not manually patch the final assistant response"));
    }

    #[test]
    fn generic_content_handles_stale_duplicate_instructions() {
        let content = super::content_for_env(Environment::Generic);

        assert!(content.contains("Claude Code: `/agent-doc <FILE>`"));
        assert!(content.contains("Codex: `agent-doc <FILE>`"));
        assert!(content.contains("OpenCode: `/agent-doc <FILE>`"));
        assert!(content.contains("agent-doc skill install --harness claude --reload restart"));
        assert!(content.contains("agent_doc_auto_compact"));
        assert!(content.contains("Codex `agent-doc skill install --harness codex`"));
        assert!(content.contains("agent-doc skill install --harness opencode"));
        assert!(content.contains("stale duplicate instructions"));
        assert!(content.contains("continue with the task"));
    }

    #[test]
    fn generated_harness_content_shares_hot_path_outside_invocation() {
        let claude = super::content_for_env(Environment::ClaudeCode);
        let codex = super::content_for_env(Environment::Codex);
        let opencode = super::content_for_env(Environment::OpenCode);

        let claude_shared = super::remove_markdown_section(&claude, "## Invocation");
        let codex_shared = super::remove_markdown_section(&codex, "## Invocation");
        let opencode_shared = super::remove_markdown_section(&opencode, "## Invocation");
        let claude_shared = super::remove_markdown_section(
            &claude_shared,
            "## Auto-loop while queue is active (Claude Code)",
        );
        let claude_shared = claude_shared.replace(CLAUDE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let codex_shared = codex_shared.replace(CODEX_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let opencode_shared = opencode_shared.replace(OPENCODE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");

        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            codex_shared.replace(CODEX_DESCRIPTION, "<DESC>")
        );
        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            opencode_shared.replace(OPENCODE_DESCRIPTION, "<DESC>")
        );
    }

    #[test]
    fn bundled_runbooks_include_harness_invocation() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "harness-invocation.md"),
            "harness-invocation.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_dynamic_context() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "dynamic-context.md"),
            "dynamic-context.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            SKILL_TEMPLATE.contains("dynamic-context.md"),
            "SKILL.md should list dynamic-context in the runbook catalog"
        );
    }

    #[test]
    fn bundled_okf_includes_agent_doc_concepts() {
        assert!(
            BUNDLED_OKF.iter().any(|(name, _)| *name == "index.md"),
            "index.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, _)| *name == "session-cycle.md"),
            "session-cycle.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, _)| *name == "instruction-surface.md"),
            "instruction-surface.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, content)| *name == "dynamic-context.md"
                    && content.contains("Dynamic Context")),
            "dynamic-context.md should be in BUNDLED_OKF"
        );
    }

    #[test]
    fn bundled_runbooks_include_commit_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "commit.md"),
            "commit.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_jb_cache_conflict_runbook() {
        assert!(
            BUNDLED_RUNBOOKS.iter().any(|(name, content)| {
                *name == "jb-cache-conflict.md" && content.contains("## Quiescent CRDT Delivery")
            }),
            "jb-cache-conflict.md and its CRDT convergence contract should be bundled"
        );
        assert!(
            SKILL_TEMPLATE.contains("jb-cache-conflict"),
            "SKILL.md should list jb-cache-conflict in the runbook catalog"
        );
    }

    #[test]
    fn bundled_runbooks_include_command_synonyms_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "command-synonyms.md"),
            "command-synonyms.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "compound-task-steering.md"),
            "compound-task-steering.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "planning-dispatch.md"),
            "planning-dispatch.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_split_spec_files_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "split-spec-files.md"),
            "split-spec-files.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_baseline_drift_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "baseline-drift.md"),
            "baseline-drift.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            SKILL_TEMPLATE.contains("baseline-drift"),
            "SKILL.md should list baseline-drift in the runbook catalog"
        );
    }

    #[test]
    fn harness_invocation_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");
        assert!(content.contains("## Directive Semantics"));
        assert!(content.contains(
            "Imperative user edits inside an `agent-doc` session document are executable directives"
        ));
        assert!(content.contains("Do **not** emit status-only progress prose while doing neither"));
        assert!(content.contains("## Manual Repair Default"));
        assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
        assert!(content.contains("Claude Code"));
        assert!(content.contains("Codex"));
        assert!(content.contains("OpenCode"));
        assert!(content.contains("Harness Detection"));
        assert!(content.contains("Response Header Attribution"));
        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("### Re: topic — opus-4-6"));
        assert!(content.contains("### Re: topic — codex"));
        assert!(content.contains("### Re: topic — claude"));
        assert!(content.contains("Manual repair / missed patchback"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(
            content.contains(
                "Do not patch the document early and then keep working for the same turn"
            )
        );
        assert!(
            content.contains("the manual repo commit must exclude the active session document")
        );
        assert!(content.contains("Resolve the intended non-session path set first"));
        assert!(content.contains("stop immediately on any stage failure"));
        assert!(content.contains("verify the staged diff still matches the intended set"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains("UserPromptSubmit"));
        assert!(content.contains("agent-doc hook codex-stop"));
    }

    #[test]
    fn harness_invocation_runbook_opencode_section_requires_session_check() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");

        let opencode_start = content
            .find("## OpenCode")
            .expect("## OpenCode section not found");
        let next_section = content[opencode_start + 1..]
            .find("\n## ")
            .map(|i| opencode_start + 1 + i)
            .unwrap_or(content.len());
        let opencode_section = &content[opencode_start..next_section];

        assert!(
            opencode_section.contains("agent-doc session-check"),
            "OpenCode section must require session-check after finalize: {opencode_section}"
        );
        assert!(
            opencode_section.contains("Fail closed"),
            "OpenCode section must include fail-closed guard: {opencode_section}"
        );
        assert!(
            opencode_section.contains("response text visible in the console but absent"),
            "OpenCode section must name the CLI-only-output anti-pattern: {opencode_section}"
        );
    }

    #[test]
    fn commit_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "commit.md")
            .expect("commit.md not found");
        assert!(content.contains(
            "Every appended `agent-doc` session response is one complete, validated write+commit"
        ));
        assert!(content.contains("agent-doc respond <FILE>"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("keep the active session document out of that manual git commit"));
        assert!(content.contains("Resolve the exact intended non-session path set first"));
        assert!(content.contains("verify `git diff --cached --name-only`"));
        assert!(content.contains(
            "Do **not** continue to `git commit` after a narrowed `git add` / stage failure"
        ));
        assert!(content.contains(
            "Do **not** stage the active session document into an ordinary repo `git commit`"
        ));
    }

    #[test]
    fn compound_task_runbook_defers_session_doc_commit_until_finalize() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains(
            "run `agent-doc finalize` / `write --commit` so the session document gets its own binary-owned closeout commit"
        ));
        assert!(content.contains(
            "validate and commit only the intended non-session repo files, finalize the session document, then push"
        ));
        assert!(content.contains("stop on any stage failure"));
    }

    #[test]
    fn compact_exchange_runbook_content_uses_binary_owned_closeout() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compact-exchange.md")
            .expect("compact-exchange.md not found");
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("binary-owned continuation"));
        assert!(content.contains("VCS refresh signal"));
        assert!(content.contains("agent:backlog"));
        assert!(content.contains("agent:queue"));
        assert!(content.contains("agent:icebox"));
        assert!(content.contains("prompt_presets"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_backlog_capture_rules() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("beginning of the backlog"));
        assert!(content.contains("[recommended]"));
        assert!(content.contains("preserve the order you presented them in"));
        assert!(content.contains("follow-on step from an ordered batch"));
        assert!(content.contains("--backlog-reorder gkke,9pw9,step3"));
        assert!(content.contains("Existing `do #id` work that completed this cycle"));
        assert!(content.contains("--done <id>"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_custom_id_docs() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("id=<custom>"));
        assert!(content.contains("id=#spec1"));
        assert!(content.contains("ASCII alphanumeric"));
    }

    #[test]
    fn pending_ops_runbook_content_covers_plan_backed_items() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("create the plan file"));
        assert!(content.contains("include that exact plan"));
        assert!(content.contains("file path in the item text"));
        assert!(content.contains("plan-spec2-rollout.md"));
        assert!(content.contains("one flush-left backlog item per"));
        assert!(content.contains("queue entries and closeouts should target"));
    }

    #[test]
    fn command_synonyms_runbook_content_covers_orchestrate_modes() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "command-synonyms.md")
            .expect("command-synonyms.md not found");
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode parallel"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode dag"));
        assert!(content.contains("Run these in order"));
        assert!(content.contains("fan out"));
        assert!(content.contains("after X do Y"));
        assert!(content.contains("default to `--mode sequential`"));
    }

    #[test]
    fn compound_task_steering_runbook_covers_explicit_normalization() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains("do #ntoc. Add to today's news. commit + push"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("Do not invent binary-owned prose grammar"));
        assert!(content.contains("commit + push"));
        assert!(content.contains("Preserve it. Do not rewrite explicit orchestration"));
    }

    #[test]
    fn planning_dispatch_runbook_content_covers_plan_contract() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "planning-dispatch.md")
            .expect("planning-dispatch.md not found");
        assert!(content.contains("agent-doc plan <FILE>"));
        assert!(content.contains("prompt_targets"));
        assert!(content.contains("repo_actions"));
        assert!(content.contains("required_commands"));
        assert!(content.contains("pending_mutations"));
        assert!(content.contains("handoff"));
        assert!(content.contains("blockers"));
        assert!(content.contains("handoff=orchestrate"));
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install_test(Some(dir.path())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn installed_harness_skill_content_shares_completion_boundary_text() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_skill_for_env(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();

        let claude =
            std::fs::read_to_string(dir.path().join(".claude/skills/agent-doc/SKILL.md")).unwrap();
        let opencode =
            std::fs::read_to_string(dir.path().join(".opencode/skills/agent-doc/SKILL.md"))
                .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/skills/agent-doc/SKILL.md")).unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("agent-doc respond <FILE>"));
            assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
            assert!(content.contains("requires the cycle to reach `committed`"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("final document-mutation boundary for the cycle"));
            assert!(content.contains("Imperative edits are executable directives"));
            assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
            assert!(content.contains("Agent harnesses own full-suite verification"));
            assert!(content.contains("Do not waive red suites as \"unrelated\" or \"flaky\""));
            assert!(content.contains("Do not rely on a pre-commit hook"));
        }
        assert!(claude.contains("final document-mutation boundary for the cycle"));
        assert!(codex.contains(".codex/hooks.json"));
        assert!(codex.contains("fail-closed backstop"));
    }
}
