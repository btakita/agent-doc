//! # Module: frontmatter
//!
//! ## Spec
//! - Defines the YAML frontmatter schema (`Frontmatter`) embedded between `---\n` delimiters at
//!   the top of agent-doc documents.
//! - `parse(content)` splits a document string into `(Frontmatter, body_str)`. Documents that do
//!   not start with `---\n` return a default `Frontmatter` and the full content as body.
//!   A UTF-8 BOM or horizontal whitespace before the opening marker is tolerated so
//!   editor-created session docs do not silently become unmanaged.
//!   Unterminated frontmatter (no closing `---`) returns `Err`.
//! - `parse_for_file(content, file)` is the path-aware variant: it wraps YAML parse failures with
//!   the document path, resolves project-local required SSH defaults from `.agent-doc/config.toml`,
//!   and fails closed when a configured SSH-dependent document cannot resolve any targets.
//! - `write(fm, body)` serialises `Frontmatter` to YAML and prepends it to `body`, producing a
//!   complete document string.
//! - `set_session_id` / `set_resume_id` are convenience wrappers: parse → mutate one field →
//!   write. They create frontmatter when none exists.
//! - `ensure_session` is idempotent: if `session` is already set the document is returned
//!   unchanged; otherwise a UUID v4 is generated and written.
//! - `set_format_and_write` atomically sets `agent_doc_format` + `agent_doc_write` and clears
//!   the deprecated `agent_doc_mode` field.
//! - `merge_fields(content, yaml_fields)` applies additive key/value patches to the frontmatter;
//!   unknown keys are logged to stderr and discarded; the document body is preserved unchanged.
//! - `set_tmux_session` is retained for backward compatibility; `tmux_session` in frontmatter is
//!   deprecated — runtime session is now determined by `--window` or `current_tmux_session()`.
//! - `AgentDocFormat` (append | template) controls document structure; serialised as `inline` /
//!   `template` in YAML; `inline` and `append` are accepted as aliases on deserialise.
//! - `AgentDocWrite` (merge | crdt) controls the write/merge strategy.
//! - `ResolvedMode` is the canonical (format, write) pair after deprecation migration.
//!   `Frontmatter::resolve_mode()` applies a three-level priority: explicit `agent_doc_format` /
//!   `agent_doc_write` fields > deprecated `agent_doc_mode` string > defaults
//!   (format=template, write=crdt).
//! - Deprecated `agent_doc_mode` values: `"append"` → format=Append; `"template"` or `"stream"`
//!   → format=Template; all keep write=Crdt.
//! - `StreamConfig` carries optional CRDT stream parameters: write-back interval, ANSI stripping,
//!   target component, and chain-of-thought routing.
//! - `prompt_presets` stores reusable named prompt snippets for orchestrated tasks and round-trips
//!   exact preset names/values, including symbolic keys such as `#1`.
//! - `codex_network_access` stores an explicit Codex network policy (`inherit`, `enabled`,
//!   `disabled`) so agent-doc can own the `CODEX_SANDBOX_NETWORK_DISABLED` contract instead of
//!   silently inheriting an ambient launcher override.
//! - Field renames and aliases ensure backward compatibility:
//!   `session` / `agent_doc_session` → `session`;
//!   `mode` / `response_mode` / `agent_doc_mode` → `mode`.
//!
//! ## Agentic Contracts
//! - `parse` never panics; it returns `Err` only for an unterminated frontmatter block or a YAML
//!   deserialisation failure. All missing optional fields default to `None`.
//! - `write(parse(doc).0, parse(doc).1)` round-trips the document: re-parsing the result yields
//!   semantically equivalent `Frontmatter` and an identical body string, without accumulating
//!   blank lines at the start of the body.
//! - `ensure_session` is idempotent: calling it twice on the same document returns the same
//!   session ID and does not modify the document on the second call.
//! - `merge_fields` is additive: it never removes existing fields unless an explicit `null`
//!   value is supplied for that field in `yaml_fields`.
//! - `set_format_and_write` is the only function that actively clears a field (`mode = None`);
//!   all other helpers are strictly additive.
//! - `resolve_mode` is pure and total: it always returns a valid `ResolvedMode` regardless of
//!   which combination of deprecated and current fields are populated.
//! - `prompt_presets` is deterministic: parse/write preserves preset names and bodies exactly,
//!   including multiline values and symbolic names like `#1`.
//! - `codex_network_access` round-trips as a stable enum and is preserved across
//!   parse/write/merge cycles.
//! - Serialised YAML always uses the canonical field names (`agent_doc_session`,
//!   `agent_doc_mode`, `agent_doc_format`, `agent_doc_write`); legacy aliases are read-only.
//!
//! ## Evals
//! - parse_no_frontmatter: content without `---` prefix → default Frontmatter, full content as body
//! - parse_all_fields: document with all known fields → each field correctly populated
//! - parse_null_fields: YAML `null` values → all fields `None`
//! - parse_unterminated_frontmatter: `---` open with no close → `Err` "Unterminated frontmatter"
//! - parse_closing_at_eof: closing `---` without trailing newline → body = ""
//! - write_roundtrip: write then parse → identical Frontmatter and identical body
//! - repeated_parse_write_roundtrip_preserves_body_prefix: repeated parse/write cycles preserve
//!   the exact body prefix and do not accumulate blank lines before the first heading
//! - write_default_frontmatter: default Frontmatter → output starts with `---\n`, ends with `---\n`
//! - set_session_id_creates_frontmatter: plain doc → frontmatter added with correct session
//! - set_session_id_preserves_other_fields: existing fields not clobbered by session update
//! - ensure_session_no_frontmatter: plain doc → UUID generated, written, returned
//! - ensure_session_existing_session: session already set → content unchanged, same ID returned
//! - parse_legacy_session_field: `session:` alias → populates `fm.session`
//! - parse_mode_shorthand_alias: `mode:` alias → populates `fm.mode`
//! - write_uses_agent_doc_mode_field: deprecated mode serialised as `agent_doc_mode:`, not `mode:`
//! - write_uses_new_field_name: session serialised as `agent_doc_session:`, not `session:`
//! - resolve_mode_defaults: empty Frontmatter → format=Template, write=Crdt
//! - resolve_mode_new_fields_override_deprecated: both present → new fields win
//! - set_format_and_write_clears_deprecated_mode: deprecated `mode` cleared after migration
//! - merge_fields_adds_new_field: new key added without disturbing existing fields
//! - merge_fields_ignores_unknown: unknown key logged to stderr, not stored
//! - parse_prompt_presets_roundtrip: preset map with multiline values → parse/write preserves keys
//!   and bodies exactly
//! - parse_codex_network_access_field: `codex_network_access: enabled` → enum value
//! - resolve_harness_model: harness-specific model fields override `model`
//!   for `claude-code`, `codex`, and `opencode`
//! - parse_harness_model_fields: harness model fields round-trip through parse/write
//! - merge_fields_claude_model / merge_fields_codex_model / merge_fields_opencode_model:
//!   merge patches for harness model fields

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use agent_doc_model_tier::Tier;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationMode {
    #[default]
    SingleUser,
    Shared,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexNetworkAccess {
    #[default]
    Inherit,
    Enabled,
    Disabled,
}

/// Canonical queue activation control (`#queue-state-unify`).
///
/// The `queue:` frontmatter key subsumes the deprecated `queue_active:` boolean
/// and the `auto` attribute on the `agent:queue` marker. `start` (alias `go`)
/// activates the queue; `stop` deactivates it. Parsed leniently (case- and
/// whitespace-insensitive); unknown values resolve to `None` and are ignored so
/// a typo never silently flips activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueControl {
    /// `start` / `go` — activate the queue.
    Start,
    /// `stop` — deactivate the queue.
    Stop,
}

impl QueueControl {
    /// Parse a raw `queue:` value. `start`/`go` → [`Start`], `stop` → [`Stop`].
    /// Returns `None` for empty/unknown values.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "start" | "go" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    /// Whether this control activates the queue.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Start)
    }
}

/// Document format: controls document structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentDocFormat {
    /// Alternating ## User / ## Assistant blocks (also known as "inline")
    #[clap(alias = "inline")]
    Append,
    /// In-place component patching with <!-- agent:name --> markers
    Template,
}

impl fmt::Display for AgentDocFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Append => write!(f, "inline"),
            Self::Template => write!(f, "template"),
        }
    }
}

impl Serialize for AgentDocFormat {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Append => serializer.serialize_str("inline"),
            Self::Template => serializer.serialize_str("template"),
        }
    }
}

impl<'de> Deserialize<'de> for AgentDocFormat {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "append" | "inline" => Ok(Self::Append),
            "template" => Ok(Self::Template),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["inline", "append", "template"],
            )),
        }
    }
}

/// Write strategy: controls how responses are merged into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AgentDocWrite {
    /// 3-way merge via git merge-file
    Merge,
    /// CRDT-based conflict-free merge (yrs)
    Crdt,
}

impl fmt::Display for AgentDocWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Merge => write!(f, "merge"),
            Self::Crdt => write!(f, "crdt"),
        }
    }
}

/// Resolved mode pair — the canonical representation after deprecation migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedMode {
    pub format: AgentDocFormat,
    pub write: AgentDocWrite,
}

impl ResolvedMode {
    pub fn is_template(&self) -> bool {
        self.format == AgentDocFormat::Template
    }

    pub fn is_append(&self) -> bool {
        self.format == AgentDocFormat::Append
    }

    pub fn is_crdt(&self) -> bool {
        self.write == AgentDocWrite::Crdt
    }
}

/// Configuration for stream mode (real-time CRDT write-back).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Write-back interval in milliseconds (default: 200)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    /// Strip ANSI escape codes from agent output (default: true)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_ansi: Option<bool>,
    /// Target component name for stream output (default: "exchange")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Include chain-of-thought (thinking) blocks in output (default: false)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    /// Route thinking to a separate component (e.g., "log"). If unset, thinking
    /// is interleaved with response text in the target component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_target: Option<String>,
    /// Maximum lines for console capture (default: 50). Limits the content
    /// written to the console component to prevent indefinite growth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<usize>,
}

/// Live finalize-pipeline state tracker (`#fm-run-id-step`, Phase 5 of
/// `tasks/agent-doc/plans/reentrant-finalize-pipeline.md`).
///
/// Surfaces the current cycle's identity and step directly in frontmatter so any
/// `agent-doc` invocation — or an editor plugin / JB `Run Agent Doc` action — can
/// read where a crashed or resumed cycle left off without replaying the git log or
/// parsing `.agent-doc/cycle_state`.
///
/// This is ephemeral runtime state, not user configuration: the orchestration layer
/// writes it at each phase transition and clears it once the cycle reaches a terminal
/// phase. `step` is a free-form string (the orchestration `CyclePhase` rendered
/// lowercase, e.g. `preflight_started` / `response_captured` / `write_applied` /
/// `committed`) so frontmatter parsing round-trips newer phase names without data
/// loss, and so this focused crate need not depend on the orchestration
/// `CyclePhase` enum.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDocPipeline {
    /// Logical turn identifier — the backlog/queue task ID driving this cycle
    /// (e.g. `#fm-run-id-step`), or an auto-generated id when the queue prompt has
    /// no backlog match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Cycle-state run identifier (the `cycle-<millis>` capture/cycle id) for
    /// correlating frontmatter state with `.agent-doc/captures` + cycle_state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Current pipeline step — the orchestration `CyclePhase` rendered as a
    /// lowercase string. Free-form so a newer phase round-trips through an older
    /// core build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Backlog ID for the queued task, when the driving queue prompt resolved to
    /// (or created) a tracked backlog item. Distinct from `turn_id` only when the
    /// turn id is auto-generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_task_id: Option<String>,
}

impl AgentDocPipeline {
    /// True when no field is populated. Used by `skip_serializing_if` so a drained
    /// or terminal cycle leaves no `agent_doc_pipeline:` block behind in the document.
    pub fn is_empty(&self) -> bool {
        self.turn_id.is_none()
            && self.run_id.is_none()
            && self.step.is_none()
            && self.queue_task_id.is_none()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingCaptureGuardMode {
    #[default]
    Warn,
    #[serde(alias = "error")]
    Strict,
    Off,
}

/// Lint dialect mode for the `tagpath lint --dialect agent-doc` finalize gate.
///
/// Resolution precedence: CLI `--lint=...` > frontmatter
/// `agent_doc_lint_dialect` > workspace `.agent-doc/config.toml` `[lint]
/// dialect` > default (`warn`).
///
/// - `Warn` (default): lint runs; errors block the cycle, warnings surface on
///   stderr but do not block.
/// - `Strict`: warnings are promoted to errors.
/// - `Off`: lint gate is skipped entirely. Used to migrate documents with
///   historical malformed directives while staging fixes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LintDialectMode {
    #[default]
    Warn,
    #[serde(alias = "error")]
    Strict,
    Off,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FreeTextExecutionMode {
    /// Prefer `/goal` when the active harness supports it; otherwise use queue work.
    #[default]
    Auto,
    /// Use a native `/goal` command when available, falling back to queue work.
    Goal,
    /// Always execute admitted free-text work through `agent:queue`.
    Queue,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Document/routing UUID — permanent identifier for tmux pane routing.
    /// Serialized as `agent_doc_session` in YAML; reads legacy `session` via alias.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_session",
        alias = "session"
    )]
    pub session: Option<String>,
    /// Agent conversation ID — used for `--resume` with agent backends.
    /// Separate from `session` so the routing key never changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Per-harness model override for Claude Code sessions.
    /// When running under Claude Code, this takes precedence over `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_model: Option<String>,
    /// Per-harness model override for Codex sessions.
    /// When running under Codex, this takes precedence over `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_model: Option<String>,
    /// Per-harness model override for OpenCode sessions.
    /// When running under OpenCode, this takes precedence over `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// **Deprecated.** Tmux session name for pane affinity (e.g., "claude").
    /// Session is now determined at runtime by `--window` argument (sync) or
    /// `current_tmux_session()` (route/start). Still read for backward compatibility
    /// and auto-repaired by sync when it differs from the context session.
    /// Will be removed in a future version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_session: Option<String>,
    /// **Deprecated.** Use `agent_doc_format` + `agent_doc_write` instead.
    /// Kept for backward compatibility. Values: "append", "template", "stream".
    /// Serialized as `agent_doc_mode` in YAML; reads legacy `response_mode` and shorthand `mode` via aliases.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_mode",
        alias = "mode",
        alias = "response_mode"
    )]
    pub mode: Option<String>,
    /// Document format: controls document structure (append | template).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_format"
    )]
    pub format: Option<AgentDocFormat>,
    /// Write strategy: controls merge behavior (merge | crdt).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_write"
    )]
    pub write_mode: Option<AgentDocWrite>,
    /// Stream mode configuration (used when write strategy is CRDT).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_stream"
    )]
    pub stream_config: Option<StreamConfig>,
    /// Additional CLI arguments to pass to the agent process (harness-neutral).
    /// Takes precedence over harness-specific aliases. Space-separated string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_args: Option<String>,
    /// Additional CLI arguments to pass to the `claude` process.
    /// Backward-compatible alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_args: Option<String>,
    /// Additional CLI arguments to pass to the `codex` process.
    /// Codex-only alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_args: Option<String>,
    /// Additional CLI arguments to pass to the `opencode` process.
    /// OpenCode-only alias for `agent_args`. `agent_args` takes precedence when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_args: Option<String>,
    /// Explicit Codex network policy. `inherit` keeps the launcher's ambient
    /// `CODEX_SANDBOX_NETWORK_DISABLED` setting, `enabled` removes it, and
    /// `disabled` forces it on for the child session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_network_access: Option<CodexNetworkAccess>,
    /// Required SSH targets that must be reachable for Codex-backed work to
    /// proceed. When present, agent-doc probes these targets before trusting a
    /// resumed Codex session and fails closed when the capability cannot be
    /// proven.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_ssh_targets: Vec<String>,
    /// Optional profile name resolved via project-local `.agent-doc/config.toml`
    /// SSH mappings when the document omits direct `required_ssh_targets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_ssh_profile: Option<String>,
    /// Maximum number of managed-capability-proof attempts before the dispatch
    /// gate is set to `Failed`. A transient network blip is retried (with
    /// backoff) up to this many times so it no longer permanently wedges the
    /// session. `1` disables retry (legacy behavior). Default `3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_proof_max_attempts: Option<u32>,
    /// Base back-off (seconds) between managed-capability-proof retries. The
    /// delay grows exponentially from this value, capped internally. Default `2`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_proof_retry_backoff_secs: Option<u64>,
    /// Override for the managed-capability child network/writable-root probe
    /// timeout (seconds). Slow-network environments can raise it above the
    /// default `45`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_proof_probe_timeout_secs: Option<u64>,
    /// When true, passes `--no-mcp` to the `claude` process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_mcp: Option<bool>,
    /// When true, passes `--enable-tool-search` to the `claude` process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_tool_search: Option<bool>,
    /// Debounce duration in milliseconds for preflight mtime settling.
    /// Default: 2000ms. Set to 0 to disable debounce (run immediately).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_debounce"
    )]
    pub debounce_ms: Option<u64>,
    /// Linked resources: local file paths or URLs (relative to this document's directory).
    /// Changes in linked docs are surfaced during preflight.
    #[serde(default, skip_serializing_if = "Vec::is_empty", alias = "related_docs")]
    pub links: Vec<String>,
    /// Explicit opt-in for automatic compaction/reload policies.
    /// Session-accretion heuristics never compact by themselves; omit to disable.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_auto_compact"
    )]
    pub auto_compact: Option<usize>,
    /// Explicit opt-in for the supervisor idle-queue watch to pre-emptively
    /// interleave a context-clear (`/clear`) before an active queue head when
    /// session accretion is unhealthy (`#nm1x-no-preempt-clear`). Off by default.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_queue_context_reset"
    )]
    pub queue_context_reset: Option<bool>,
    /// Explicit opt-in for opportunistic gated-review auto-verification
    /// (`#optverify`). When true, preflight may flip a gated `[/]` review item
    /// carrying a verify predicate to `[x]` once its proof marker is provable
    /// from `ops.log` (and no disproof marker fired). Off by default — without
    /// the opt-in, preflight only *surfaces* the provable/failed status and
    /// never silently flips a human gate.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_gate_autoverify"
    )]
    pub gate_autoverify: Option<bool>,
    /// Context-usage percentage (0–100) at or above which an opted-in editor
    /// (JB `Run Agent Doc`) pre-emptively runs `/clear` before the next dispatch
    /// (`#clear-opt-in-threshold`). Resolution: this frontmatter value, then the
    /// project config `.agent-doc/config.toml`, then the built-in default of 50.
    /// The threshold value is owned by the binary so all editors share it; the
    /// context-percentage comparison and the actual pre-emptive clear are the
    /// editor's responsibility.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_clear_threshold"
    )]
    pub clear_threshold: Option<u8>,
    /// Explicit per-document opt-in/opt-out for supervisor auto-recycle
    /// (`#ctlrecycle` R3 / `#suprecyclequeue`). When the route-owned supervisor
    /// detects it is running a stale binary at an idle / inter-queue-item
    /// boundary, `true` lets it `execve` hot-reload onto the freshly-installed
    /// build (preserving the live agent child + pane); `false` force-disables
    /// that swap so a stale supervisor only logs `supervisor_binary_stale_detected`.
    /// Resolution: env `AGENT_DOC_SUPERVISOR_AUTO_RECYCLE`, then this frontmatter
    /// value, then the project config `agent_doc_supervisor_auto_recycle`, then
    /// the built-in default of ON.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_supervisor_auto_recycle"
    )]
    pub supervisor_auto_recycle: Option<bool>,
    /// Explicit per-document opt-in/opt-out for restart-agent on a harness change
    /// (`#agentreloadrestart`). When the resolved harness from current frontmatter
    /// (`agent:`) differs from the one the running supervisor launched with,
    /// `true` lets the supervisor auto-restart the agent at the next idle/turn
    /// boundary so changing `agent` (e.g. claude→opencode) takes effect without a
    /// manual restart; `false` disables that auto-restart (the change then applies
    /// only on the next manual restart). Resolution: env
    /// `AGENT_DOC_AGENT_CHANGE_RESTART`, then this frontmatter value, then the
    /// project config `agent_doc_agent_change_restart`, then the built-in default
    /// of ON.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_agent_change_restart"
    )]
    pub agent_change_restart: Option<bool>,
    /// Explicit per-document opt-in/opt-out for supervisor auto-install
    /// (`#supautoinstall`). When a DOGFOODING supervisor (an agent-doc session
    /// editing agent-doc's own source) finds the source newer than the installed
    /// binary at an idle boundary, `true` lets it `cargo build --release` +
    /// `cargo install` + `agent-doc lib-install` so the `#ctlrecycle` recycle then
    /// hot-reloads onto it; `false` force-disables that build so it only logs
    /// `supervisor_source_newer_detected`. Resolution: env
    /// `AGENT_DOC_SUPERVISOR_AUTO_INSTALL`, then this frontmatter value, then the
    /// project config `agent_doc_supervisor_auto_install`, then the built-in
    /// default of ON. Never fires for a non-agent-doc dogfood session document,
    /// even when this field is `true`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_supervisor_auto_install"
    )]
    pub supervisor_auto_install: Option<bool>,
    /// Required model tier for this document. When set, preflight emits this as
    /// `required_tier`, which the skill uses as a hard gate: if the running model's
    /// tier is below this value, the skill writes a switch prompt and stops.
    /// Values: `auto | low | med | high`. Default: absent (no gate).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_model_tier"
    )]
    pub model_tier: Option<Tier>,
    /// Pending-capture guard mode for `session-check`.
    /// Values: `warn` (default when absent), `strict`, `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_capture_guard: Option<PendingCaptureGuardMode>,
    /// Pending-done guard mode for `session-check`.
    /// Values: `warn` (default when absent), `strict`, `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_done_guard: Option<PendingCaptureGuardMode>,
    /// Review-done guard mode for `--done`.
    /// Values: `off` (default when absent), `warn`, `strict`/`error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_done_guard: Option<PendingCaptureGuardMode>,
    /// When true, closeout may auto-apply `--done` for clear completion signals.
    /// Default is absent/false; explicit `--done` and explicit `#id done`
    /// directives still work without this opt-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_done: Option<bool>,
    /// Lint dialect mode for the agent-doc finalize lint gate. Values:
    /// `warn` (default), `strict`, `off`. See `lint_gate` module for the
    /// full precedence chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_doc_lint_dialect: Option<LintDialectMode>,
    /// Document-level lifecycle hooks: shell commands executed at key events.
    ///
    /// Supported events: `session_start`, `post_write`, `post_commit`.
    /// Each event maps to a list of shell commands (run via `sh -c`).
    ///
    /// Template variables substituted before execution:
    /// - `{{session_id}}` — document session UUID
    /// - `{{file}}` — document file path
    /// - `{{agent}}` — agent name (or empty string)
    /// - `{{model}}` — model name (or empty string)
    ///
    /// Execution is best-effort: failures log to stderr and never block the session.
    ///
    /// Example:
    /// ```yaml
    /// hooks:
    ///   session_start:
    ///     - "curl -s -X POST https://api.example.com/sessions -d '{\"session_id\": \"{{session_id}}\"}'"
    ///   post_write:
    ///     - "notify-send 'agent-doc' 'Response written to {{file}}'"
    /// ```
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub hooks: std::collections::HashMap<String, Vec<String>>,
    /// Environment variables to inject into the Claude session.
    /// Keys are env var names, values are strings supporting shell expansion
    /// (`$(command)`, `$VAR`, `${VAR}`). Order is preserved: later values can
    /// reference earlier keys. A `null` value (YAML `KEY: null`) means "unset
    /// this key in the child env" — supported by every consumer (start.rs,
    /// run.rs, stream.rs, parallel.rs, supervisor/env.rs).
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Option<String>>,
    /// Reusable prompt snippets that can be expanded into orchestrated task prompts.
    ///
    /// Example:
    /// ```yaml
    /// prompt_presets:
    ///   "#1": |
    ///     Today is 2026-04-25.
    ///     Keep the work tree clean.
    /// ```
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub prompt_presets: indexmap::IndexMap<String, String>,
    /// How free-text work admitted from `agent:exchange` or `agent:queue` should be
    /// executed after the binary creates backlog items. Values: `auto`, `goal`,
    /// `queue`. Project/global configs use the same key.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_free_text_execution",
        alias = "free_text_execution"
    )]
    pub free_text_execution: Option<FreeTextExecutionMode>,
    /// Lower-agent dispatch policy for planning/job-packet generation.
    /// Values are currently interpreted by `plan` as manual/auto/off text;
    /// unknown values round-trip so newer policy names do not break older binaries.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_dispatch"
    )]
    pub dispatch: Option<String>,
    /// Whether the supervisor starts from the parent process env before
    /// applying the `env:` overlay. Default `true` — the supervised claude
    /// child inherits PATH/HOME/TERM/TMUX/... from the shell that launched
    /// agent-doc, then frontmatter `env:` overrides/unsets specific keys.
    /// Set `false` for a sealed env (only the explicit `env:` keys, nothing
    /// from the parent). Consumed by `supervisor/env.rs::EnvSpec`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_doc_env_inherit: Option<bool>,
    /// Explicit working directory for the supervised claude child process.
    /// Resolved relative to the document's parent directory if a relative path
    /// is given. When absent, the supervisor falls back to project-root detection
    /// (`.agent-doc/` walk) or the document's parent directory.
    /// See `src/agent-doc/specs/supervisor.md` § Core Invariants / CWD determinism.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_cwd"
    )]
    pub cwd: Option<String>,
    /// Canonical queue activation control (`#queue-state-unify`).
    /// `start` (alias `go`) activates the queue; `stop` deactivates it. This is
    /// the preferred surface and subsumes the deprecated `queue_active:` boolean
    /// and the `auto` marker attribute. [`normalize_queue_control`] folds an
    /// explicit value onto `queue_active` at parse time so every existing reader
    /// honors it; the canonical key wins over a stale `queue_active:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    /// **Deprecated** in favor of the canonical `queue:` control. Whether the
    /// queue is currently active (consuming prompts). Set to `true` when the
    /// queue is activated via `queue: start`/`go`, `auto`, a start fence, or
    /// `do queue`/`run queue`. Cleared when the queue drains to empty. Still read
    /// and written for backward compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_active: Option<bool>,
    /// Opt-in length threshold for the `#queue-prompt-echo-in-response` echo.
    /// When `Some(n)` and a consumed queue prompt is longer than `n` characters,
    /// the echo records a bounded summary (first line + truncation note + a
    /// pointer to the full `agent:queue` text) instead of copying the whole
    /// prompt verbatim. `None` (default) preserves the verbatim copy
    /// (`#queue-prompt-echo-summary`; the user deferred summarization with "for
    /// now, let's try copying the entire prompt", so it stays opt-in).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_queue_prompt_echo_max_chars"
    )]
    pub queue_prompt_echo_max_chars: Option<usize>,
    /// Collaboration scope for the document.
    /// `shared` opts the document into stricter cross-document security checks.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_collaboration"
    )]
    pub collaboration: Option<CollaborationMode>,
    /// Audit note / approval token for cross-document reads or transfers in shared mode.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_security_review"
    )]
    pub security_review: Option<String>,
    /// Live finalize-pipeline state tracker (`#fm-run-id-step`). Ephemeral runtime
    /// state written at each cycle phase transition and cleared at a terminal phase;
    /// surfaces `run_id` / `step` / `turn_id` / `queue_task_id` for crash re-entrancy
    /// and observability without a dedicated git commit. See [`AgentDocPipeline`].
    #[serde(
        default,
        skip_serializing_if = "AgentDocPipeline::is_empty",
        rename = "agent_doc_pipeline"
    )]
    pub pipeline: AgentDocPipeline,
}

/// Resolve a user-authored prompt preset reference to the frontmatter key.
///
/// Exact keys win. As a convenience for command-like preset names, a bare
/// reference such as `preset review` also resolves to a `#review` key when
/// only the hashtag form is defined.
pub fn resolve_prompt_preset_key(
    prompt_presets: &indexmap::IndexMap<String, String>,
    requested: &str,
) -> Option<String> {
    let requested = requested.trim();
    if prompt_presets.contains_key(requested) {
        return Some(requested.to_string());
    }
    if !requested.starts_with('#') {
        let hashtag_key = format!("#{requested}");
        if prompt_presets.contains_key(hashtag_key.as_str()) {
            return Some(hashtag_key);
        }
    }
    None
}

impl Frontmatter {
    /// Does this frontmatter carry any agent-doc-managed field?
    ///
    /// Used by the opt-in document gate
    /// ([`crate::project_config::is_agent_doc_document`]) to recognize a `.md`
    /// that the user clearly authored as an agent-doc session. Plain
    /// notes/README frontmatter (title/date/tags) carries none of these, so it
    /// is not auto-converted. This is the single source of truth — keep it in
    /// sync when adding new session-defining frontmatter fields.
    pub fn has_agent_doc_marker(&self) -> bool {
        self.session.is_some()
            || self.format.is_some()
            || self.write_mode.is_some()
            || self.mode.is_some()
            || self.stream_config.is_some()
            || self.agent.is_some()
            || self.resume.is_some()
            || self.model.is_some()
            || self.claude_model.is_some()
            || self.codex_model.is_some()
            || self.opencode_model.is_some()
            || self.agent_args.is_some()
            || self.claude_args.is_some()
            || self.codex_args.is_some()
            || self.opencode_args.is_some()
            || self.branch.is_some()
            || self.tmux_session.is_some()
            || self.free_text_execution.is_some()
            || self.queue_active.is_some()
            || self.dispatch.is_some()
            || self.collaboration.is_some()
            || !self.prompt_presets.is_empty()
            || !self.pipeline.is_empty()
    }

    /// Resolve the canonical (format, write) pair from all three fields.
    ///
    /// Priority:
    /// 1. Explicit `agent_doc_format` / `agent_doc_write` fields (highest)
    /// 2. Deprecated `agent_doc_mode` field (auto-migrated)
    /// 3. Defaults: format=template, write=crdt
    pub fn resolve_mode(&self) -> ResolvedMode {
        // Start with defaults
        let mut format = AgentDocFormat::Template;
        let mut write = AgentDocWrite::Crdt;

        // Apply deprecated mode if present (lowest priority)
        if let Some(ref mode_str) = self.mode {
            match mode_str.as_str() {
                "append" => {
                    format = AgentDocFormat::Append;
                    // write stays crdt (user preference: always crdt)
                }
                "template" => {
                    format = AgentDocFormat::Template;
                    // write stays crdt
                }
                "stream" => {
                    format = AgentDocFormat::Template;
                    write = AgentDocWrite::Crdt;
                }
                _ => {} // unknown mode, use defaults
            }
        }

        // Override with explicit new fields (highest priority)
        if let Some(f) = self.format {
            format = f;
        }
        if let Some(w) = self.write_mode {
            write = w;
        }

        ResolvedMode { format, write }
    }

    /// Resolve the effective model for attribution, given the active harness.
    ///
    /// Priority: harness-specific field (`claude_model` / `codex_model` / `opencode_model`)
    /// over generic `model`.
    /// Harness mapping: `"claude-code"` uses `claude_model`, `"codex"` uses `codex_model`,
    /// and `"opencode"` uses `opencode_model`.
    pub fn resolve_harness_model(&self, harness: &str) -> Option<&str> {
        let harness_specific = match harness {
            "claude-code" => self.claude_model.as_deref(),
            "codex" => self.codex_model.as_deref(),
            "opencode" => self.opencode_model.as_deref(),
            _ => None,
        };
        harness_specific.or(self.model.as_deref())
    }

    pub fn collaboration_mode(&self) -> CollaborationMode {
        self.collaboration.unwrap_or_default()
    }

    pub fn has_security_review(&self) -> bool {
        self.security_review
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    }
}

/// Fold the canonical `queue:` control onto the deprecated `queue_active`
/// boolean (`#queue-state-unify`). An explicit, recognized `queue:` value wins
/// over any stale `queue_active:` already present, so the canonical key is the
/// single source of truth. Unrecognized/empty `queue:` values leave
/// `queue_active` untouched. Callers run this immediately after deserializing
/// frontmatter so every downstream reader of `queue_active` honors `queue:`.
pub fn normalize_queue_control(fm: &mut Frontmatter) {
    if let Some(control) = fm.queue.as_deref().and_then(QueueControl::parse) {
        fm.queue_active = Some(control.is_active());
    }
}

/// Parse YAML frontmatter from a document. Returns (frontmatter, body).
/// If no frontmatter block is present, returns defaults and the full content as body.
pub fn parse(content: &str) -> Result<(Frontmatter, &str)> {
    let Some((yaml, body)) = split_frontmatter(content)? else {
        return Ok((Frontmatter::default(), content));
    };
    let mut fm: Frontmatter = serde_yaml::from_str(yaml)?;
    normalize_queue_control(&mut fm);
    Ok((fm, body))
}

/// Wrap a frontmatter parse failure with the target document display string
/// and a repair hint.
pub fn contextualize_parse_error(file_display: &str, err: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "invalid YAML frontmatter in {}: {}\n\nFix the frontmatter between the opening and closing --- markers, then rerun the command.",
        file_display,
        err
    )
}

/// Pure SSH-resolver context for [`parse_with_ssh_resolver`] / [`ensure_session_with_ssh_resolver`].
///
/// Callers in the orchestration layer pre-compute the project config + canonical
/// relative path before invoking the pure parse path, so this module never has
/// to touch `std::fs::canonicalize` or walk the project root from a `&Path`.
pub struct SshResolverContext<'a> {
    /// Resolved project config (already loaded from `.agent-doc/config.toml`).
    pub project: &'a crate::project_config::ProjectConfig,
    /// Path of the document relative to the project root, forward-slashed
    /// (e.g. `tasks/agent-doc/plan.md`). Empty string when no project root.
    pub doc_relative: &'a str,
    /// Display string for the document path — used only for error messages.
    pub file_display: &'a str,
}

/// Parse frontmatter using a pre-resolved SSH-defaults context.
///
/// Pure: no `&Path`, no `std::fs`. The file-based `parse_for_file` wrapper
/// in `agent_doc::frontmatter_io` resolves the context and delegates here.
pub fn parse_with_ssh_resolver<'a>(
    content: &'a str,
    ctx: &SshResolverContext<'_>,
) -> Result<(Frontmatter, &'a str)> {
    let Some((yaml, body)) = split_frontmatter(content)? else {
        let mut fm = Frontmatter::default();
        apply_required_ssh_defaults(&mut fm, ctx)?;
        return Ok((fm, content));
    };
    match serde_yaml::from_str(yaml) {
        Ok(mut fm) => {
            normalize_queue_control(&mut fm);
            apply_required_ssh_defaults(&mut fm, ctx)?;
            Ok((fm, body))
        }
        Err(err) => Err(contextualize_yaml_parse_error(ctx.file_display, yaml, err)),
    }
}

/// Ensure a session id using a pre-resolved SSH-defaults context.
pub fn ensure_session_with_ssh_resolver(
    content: &str,
    ctx: &SshResolverContext<'_>,
) -> Result<(String, String)> {
    let (mut fm, body) = parse_with_ssh_resolver(content, ctx)?;
    if let Some(existing) = fm.session.clone() {
        return Ok((content.to_string(), existing));
    }
    let session_id = Uuid::new_v4().to_string();
    fm.session = Some(session_id.clone());
    let updated = write(&fm, body)?;
    Ok((updated, session_id))
}

/// Write frontmatter back into a document, preserving the body.
///
/// `#writer-emits-deprecated-queue-active-false`: emit the canonical `queue:`
/// control, never the deprecated `queue_active:` line — regardless of which path
/// set the queue state. Any caller that set `queue_active` directly (then called
/// `write()`) has it folded onto `queue` here, and a `queue_active` carried over
/// from a parse (where `normalize_queue_control` mirrors `queue:` onto it) is
/// dropped so it is not double-emitted alongside the canonical key. Readers still
/// resolve both forms (`normalize_queue_control` folds `queue:` back onto
/// `queue_active` on parse), and a doc with no queue state writes neither.
pub fn write(fm: &Frontmatter, body: &str) -> Result<String> {
    let fm = if fm.queue_active.is_some() {
        let mut canonical = fm.clone();
        if canonical.queue.is_none() {
            canonical.queue = Some(
                if fm.queue_active == Some(true) {
                    "start"
                } else {
                    "stop"
                }
                .to_string(),
            );
        }
        canonical.queue_active = None;
        std::borrow::Cow::Owned(canonical)
    } else {
        std::borrow::Cow::Borrowed(fm)
    };
    let yaml = serde_yaml::to_string(fm.as_ref())?;
    Ok(format!("---\n{}---\n{}", yaml, body))
}

/// Update the session ID in a document string. Creates frontmatter if missing.
pub fn set_session_id(content: &str, session_id: &str) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.session = Some(session_id.to_string());
    write(&fm, body)
}

/// Update the resume (agent conversation) ID in a document string.
pub fn set_resume_id(content: &str, resume_id: &str) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.resume = Some(resume_id.to_string());
    write(&fm, body)
}

/// Update the live finalize-pipeline tracker in a document's frontmatter,
/// preserving every other field and the body (`#fm-run-id-step`).
///
/// Each `Some` argument overwrites that field; each `None` leaves the existing
/// value untouched. Use [`clear_pipeline_state`] to drop the whole
/// `agent_doc_pipeline:` block at a terminal cycle phase.
pub fn set_pipeline_state(
    content: &str,
    run_id: Option<&str>,
    step: Option<&str>,
    turn_id: Option<&str>,
    queue_task_id: Option<&str>,
) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    if let Some(v) = run_id {
        fm.pipeline.run_id = Some(v.to_string());
    }
    if let Some(v) = step {
        fm.pipeline.step = Some(v.to_string());
    }
    if let Some(v) = turn_id {
        fm.pipeline.turn_id = Some(v.to_string());
    }
    if let Some(v) = queue_task_id {
        fm.pipeline.queue_task_id = Some(v.to_string());
    }
    write(&fm, body)
}

/// Drop the live finalize-pipeline tracker from a document's frontmatter,
/// preserving every other field and the body (`#fm-run-id-step`).
///
/// Called at a terminal cycle phase so a drained cycle leaves no stale
/// `agent_doc_pipeline:` block behind. No-op when the block is already absent.
pub fn clear_pipeline_state(content: &str) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.pipeline = AgentDocPipeline::default();
    write(&fm, body)
}

/// `#queue-active-deprecated-line-stuck`: byte-precise removal of the deprecated
/// top-level `queue_active:` line from the leading `--- ... ---` frontmatter
/// region, but ONLY when the canonical `queue:` control is also present (so the
/// queue state is never lost). Every other byte is preserved verbatim.
///
/// This is the migration counterpart to [`merge_queue_state`]/[`write`], which
/// drop `queue_active` when they re-serialize. Those never fire on a doc whose
/// hot path preserves frontmatter byte-precisely, and the diff layer classifies
/// any `queue_active:` line as managed state (so its removal reads as a no-op and
/// is never committed) — leaving a legacy `queue_active: true|false` line stuck
/// in the file forever. Preflight repair calls this once to drop it directly on
/// disk + snapshot so the canonical `queue:` becomes the sole queue control.
/// Returns the input unchanged when there is no leading frontmatter region, no
/// `queue:` line, or no deprecated `queue_active:` line.
pub fn strip_deprecated_queue_active_line(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return content.to_string();
    }
    let Some(close_idx) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.trim_end() == "---")
        .map(|(i, _)| i)
    else {
        return content.to_string();
    };
    let fm_lines = &lines[1..close_idx];
    let has_canonical_queue = fm_lines
        .iter()
        .any(|l| l.trim_start().starts_with("queue:"));
    let has_deprecated = fm_lines
        .iter()
        .any(|l| l.trim_start().starts_with("queue_active:"));
    if !has_canonical_queue || !has_deprecated {
        return content.to_string();
    }
    let out: Vec<&str> = lines
        .iter()
        .enumerate()
        .filter(|(i, line)| {
            // Only drop the deprecated line inside the frontmatter region.
            !(*i > 0 && *i < close_idx && line.trim_start().starts_with("queue_active:"))
        })
        .map(|(_, line)| *line)
        .collect();
    out.join("\n")
}

/// #22a8 (Phase 5b write-side): byte-precise removal of the `agent_doc_pipeline:`
/// block (its key line plus indented children) from the leading `--- ... ---`
/// frontmatter region. Every other byte is preserved verbatim — unlike
/// [`clear_pipeline_state`], this never re-serializes the rest of the
/// frontmatter, so it cannot reorder or reformat other fields. Returns the input
/// unchanged when there is no leading frontmatter region or no block.
pub fn strip_pipeline_block_lines(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return content.to_string();
    }
    let Some(close_idx) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.trim_end() == "---")
        .map(|(i, _)| i)
    else {
        return content.to_string();
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut skipping = false;
    for (i, line) in lines.iter().enumerate() {
        if i == 0 || i >= close_idx {
            skipping = false;
            out.push(line);
            continue;
        }
        if skipping {
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            skipping = false;
        }
        if line.trim_start().starts_with("agent_doc_pipeline:") {
            skipping = true;
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

/// #22a8 (Phase 5b write-side): byte-precise mirror of the `agent_doc_pipeline:`
/// block into a document's frontmatter. Removes any existing block, then — when
/// `pipeline` is non-empty — renders a fresh block (via the struct's canonical
/// serializer, so `#`-prefixed ids are correctly quoted) and splices it in just
/// before the closing `---`. All other frontmatter bytes are preserved, so the
/// diff layer's matching strip cancels the write to `no_changes`. Clears the
/// block (leaving nothing behind) when `pipeline.is_empty()`.
pub fn splice_pipeline_block(content: &str, pipeline: &AgentDocPipeline) -> Result<String> {
    let stripped = strip_pipeline_block_lines(content);
    if pipeline.is_empty() {
        return Ok(stripped);
    }
    #[derive(serde::Serialize)]
    struct Wrap<'a> {
        agent_doc_pipeline: &'a AgentDocPipeline,
    }
    let rendered = serde_yaml::to_string(&Wrap {
        agent_doc_pipeline: pipeline,
    })
    .map_err(|e| anyhow::anyhow!("serialize agent_doc_pipeline block: {e}"))?;
    let block_lines: Vec<&str> = rendered.trim_end_matches('\n').split('\n').collect();

    let lines: Vec<&str> = stripped.split('\n').collect();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return Ok(stripped);
    }
    let Some(close_idx) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.trim_end() == "---")
        .map(|(i, _)| i)
    else {
        return Ok(stripped);
    };
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + block_lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i == close_idx {
            for bl in &block_lines {
                out.push((*bl).to_string());
            }
        }
        out.push((*line).to_string());
    }
    Ok(out.join("\n"))
}

/// Set both agent_doc_format and agent_doc_write, clearing deprecated agent_doc_mode.
pub fn set_format_and_write(
    content: &str,
    format: AgentDocFormat,
    write_mode: AgentDocWrite,
) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.format = Some(format);
    fm.write_mode = Some(write_mode);
    fm.mode = None;
    write(&fm, body)
}

/// Merge YAML key/value pairs into a document's frontmatter.
///
/// Takes a YAML string of fields to merge (additive — never removes keys).
/// Only known frontmatter fields are applied; unknown keys are ignored.
/// Returns the updated document content.
pub fn merge_fields(content: &str, yaml_fields: &str) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    let patch: serde_yaml::Value = serde_yaml::from_str(yaml_fields)
        .unwrap_or(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    let mapping = patch
        .as_mapping()
        .unwrap_or(&serde_yaml::Mapping::new())
        .clone();

    for (key, value) in &mapping {
        let key_str = key.as_str().unwrap_or("");
        let val_str = || value.as_str().map(|s| s.to_string());
        match key_str {
            "agent_doc_session" | "session" => fm.session = val_str(),
            "resume" => fm.resume = val_str(),
            "agent" => fm.agent = val_str(),
            "model" => fm.model = val_str(),
            "claude_model" => fm.claude_model = val_str(),
            "codex_model" => fm.codex_model = val_str(),
            "opencode_model" => fm.opencode_model = val_str(),
            "branch" => fm.branch = val_str(),
            "tmux_session" => fm.tmux_session = val_str(),
            "agent_doc_mode" | "mode" | "response_mode" => fm.mode = val_str(),
            "agent_doc_format" => {
                if let Some(s) = value.as_str()
                    && let Ok(f) = serde_yaml::from_str::<AgentDocFormat>(&format!("\"{}\"", s))
                {
                    fm.format = Some(f);
                }
            }
            "agent_doc_write" => {
                if let Some(s) = value.as_str()
                    && let Ok(w) = serde_yaml::from_str::<AgentDocWrite>(&format!("\"{}\"", s))
                {
                    fm.write_mode = Some(w);
                }
            }
            "pending_capture_guard" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<PendingCaptureGuardMode>(&format!("\"{}\"", s))
                {
                    fm.pending_capture_guard = Some(mode);
                }
            }
            "pending_done_guard" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<PendingCaptureGuardMode>(&format!("\"{}\"", s))
                {
                    fm.pending_done_guard = Some(mode);
                }
            }
            "review_done_guard" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<PendingCaptureGuardMode>(&format!("\"{}\"", s))
                {
                    fm.review_done_guard = Some(mode);
                }
            }
            "auto_done" => {
                if let Some(enabled) = value.as_bool() {
                    fm.auto_done = Some(enabled);
                }
            }
            "prompt_presets" => {
                if let Ok(presets) =
                    serde_yaml::from_value::<indexmap::IndexMap<String, String>>(value.clone())
                {
                    fm.prompt_presets = presets;
                }
            }
            "agent_doc_free_text_execution" | "free_text_execution" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<FreeTextExecutionMode>(&format!("\"{}\"", s))
                {
                    fm.free_text_execution = Some(mode);
                }
            }
            "agent_args" => fm.agent_args = val_str(),
            "claude_args" => fm.claude_args = val_str(),
            "codex_args" => fm.codex_args = val_str(),
            "opencode_args" => fm.opencode_args = val_str(),
            "codex_network_access" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<CodexNetworkAccess>(&format!("\"{}\"", s))
                {
                    fm.codex_network_access = Some(mode);
                }
            }
            "required_ssh_targets" => {
                if let Ok(targets) = serde_yaml::from_value::<Vec<String>>(value.clone()) {
                    fm.required_ssh_targets = targets;
                }
            }
            "required_ssh_profile" => fm.required_ssh_profile = val_str(),
            "queue" => {
                fm.queue = val_str();
                normalize_queue_control(&mut fm);
            }
            "queue_active" => {
                fm.queue_active = value.as_bool();
            }
            "agent_doc_collaboration" => {
                if let Some(s) = value.as_str()
                    && let Ok(mode) =
                        serde_yaml::from_str::<CollaborationMode>(&format!("\"{}\"", s))
                {
                    fm.collaboration = Some(mode);
                }
            }
            "agent_doc_security_review" => fm.security_review = val_str(),
            _ => {
                eprintln!("[frontmatter] ignoring unknown patch field: {}", key_str);
            }
        }
    }

    write(&fm, body)
}

/// Persist the canonical `queue:` activation control (`#queue-state-unify`
/// phase 4), clearing the deprecated `queue_active:` line so `queue:` is the
/// single written source of truth. `active` → `queue: start`, else `queue: stop`.
/// Reads still resolve correctly because [`normalize_queue_control`] folds
/// `queue:` back onto `queue_active` on parse.
pub fn merge_queue_state(content: &str, active: bool) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.queue = Some(if active { "start" } else { "stop" }.to_string());
    fm.queue_active = None;
    write(&fm, body)
}

/// Persist an explicit canonical `queue:` control value, clearing the deprecated
/// `queue_active:` line so the queue marker/frontmatter binding has one written
/// frontmatter source.
pub fn merge_queue_control(content: &str, control: &str) -> Result<String> {
    let normalized = match control.trim().to_ascii_lowercase().as_str() {
        "go" => "go",
        "start" => "start",
        "stop" => "stop",
        other => anyhow::bail!("unsupported queue control `{other}`"),
    };
    let (mut fm, body) = parse(content)?;
    fm.queue = Some(normalized.to_string());
    fm.queue_active = None;
    write(&fm, body)
}

/// Update the tmux_session name in a document string.
///
/// **Deprecated.** `tmux_session` in frontmatter is deprecated — session is now
/// determined at runtime. This function is retained for backward compatibility
/// (claim and sync still write it so older binaries can read it).
pub fn set_tmux_session(content: &str, session_name: &str) -> Result<String> {
    let (mut fm, body) = parse(content)?;
    fm.tmux_session = Some(session_name.to_string());
    write(&fm, body)
}

/// Ensure the document has a session ID. If no frontmatter exists, creates one
/// with a new UUID v4. If frontmatter exists but session is None/null, generates
/// a UUID and sets it. If session already exists, returns as-is.
/// Returns (updated_content, session_id).
pub fn ensure_session(content: &str) -> Result<(String, String)> {
    let (fm, _body) = parse(content)?;
    if let Some(ref session_id) = fm.session {
        // Session already set — return content unchanged
        return Ok((content.to_string(), session_id.clone()));
    }
    let session_id = Uuid::new_v4().to_string();
    let updated = set_session_id(content, &session_id)?;
    Ok((updated, session_id))
}

fn split_frontmatter(content: &str) -> Result<Option<(&str, &str)>> {
    let (opening_end, rest) = if let Some(rest) = content.strip_prefix("---\n") {
        (4, rest)
    } else {
        let without_bom = content.strip_prefix('\u{feff}').unwrap_or(content);
        let bom_len = content.len() - without_bom.len();
        let horizontal_ws_len = without_bom
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        let marker_start = bom_len + horizontal_ws_len;
        if (bom_len == 0 && horizontal_ws_len == 0) || !content[marker_start..].starts_with("---\n")
        {
            return Ok(None);
        }
        (marker_start + 4, &content[marker_start + 4..])
    };
    let (end, closing_len) = rest
        .find("\n---\n")
        .map(|end| (end, 5))
        .or_else(|| rest.find("\n---").map(|end| (end, 4)))
        .ok_or_else(|| anyhow::anyhow!("Unterminated frontmatter block"))?;
    let yaml = &rest[..end];
    let body_start = opening_end + end + closing_len; // opening marker + yaml + closing marker
    let body = if body_start <= content.len() {
        &content[body_start..]
    } else {
        ""
    };
    Ok(Some((yaml, body)))
}

/// Split a document into `(Some(yaml_without_fences), body)` or `(None, content)`.
///
/// Merge-path convenience wrapper over the internal splitter that never errors:
/// an unterminated / malformed leading block is treated as "no frontmatter" so a
/// convergence merge degrades gracefully instead of panicking. `body` is the
/// document text after the closing `---` fence (the same slice [`parse`] returns).
pub fn split_frontmatter_parts(content: &str) -> (Option<&str>, &str) {
    match split_frontmatter(content) {
        Ok(Some((yaml, body))) => (Some(yaml), body),
        _ => (None, content),
    }
}

/// Top-level frontmatter keys that agent-doc itself manages as runtime state
/// (`#fmreset`). On a *same-key* conflict — both the agent side (`ours`) and the
/// operator side (`theirs`) changed the key relative to `base` — the AGENT value
/// wins for these keys so a routine managed write (session/resume identity,
/// pipeline tracker) is not clobbered. Every other key is operator-authoritative.
/// Operator-only or agent-only edits always apply to their own key regardless of
/// this set, so an operator's frontmatter edit is never reverted.
///
/// Note: `queue` / `queue_active` are intentionally NOT here — the operator owns
/// `queue: start|stop`, and an agent-only drain flip still applies because it is
/// an agent-only change (operator did not touch it).
fn is_agent_managed_frontmatter_key(key: &str) -> bool {
    matches!(
        key,
        "agent_doc_session" | "session" | "resume" | "agent_doc_pipeline"
    )
}

/// Field-level three-way merge of the YAML frontmatter region (`#fmreset`).
///
/// Inputs are the raw YAML blocks (WITHOUT the `---` fences) for the merge base,
/// the agent side (`ours`), and the operator side (`theirs`); `None` means that
/// side had no frontmatter. Returns the merged block WITH fences and a trailing
/// newline (ready to prepend to a body), or `None` when no side had frontmatter.
///
/// Resolution per top-level key:
/// - changed on only one side → that side's value (operator edits and agent
///   managed writes both apply to their own keys; this is what stops the operator
///   frontmatter "reset");
/// - changed on both sides to the same value → that value (convergent);
/// - changed on both sides differently → operator (`theirs`) wins, EXCEPT for the
///   agent-managed keys ([`is_agent_managed_frontmatter_key`]) where the agent
///   value wins;
/// - unchanged on both → base value.
///
/// Unknown / operator-authored keys are preserved verbatim because the merge runs
/// over a `serde_yaml` mapping, never the typed [`Frontmatter`] struct (which would
/// silently drop unmodeled keys). A side with no frontmatter is treated as "did
/// not touch frontmatter" (its keys mirror base) so a transient absent side can
/// never delete operator keys. When any present side does not parse as a YAML
/// mapping, the merge falls back conservatively to the whole operator block when
/// the operator changed it, else the agent block.
pub fn merge_frontmatter_3way(
    base_yaml: Option<&str>,
    ours_yaml: Option<&str>,
    theirs_yaml: Option<&str>,
) -> Option<String> {
    if base_yaml.is_none() && ours_yaml.is_none() && theirs_yaml.is_none() {
        return None;
    }

    let wrap = |yaml: &str| {
        let trimmed = yaml.trim_end_matches('\n');
        if trimmed.is_empty() {
            "---\n---\n".to_string()
        } else {
            format!("---\n{trimmed}\n---\n")
        }
    };

    // Parse each present side as a YAML mapping. `Some(None)` = absent side;
    // `Some(Some(map))` = parsed mapping; `None` = present but not a mapping.
    let parse = |yaml: Option<&str>| -> Option<Option<serde_yaml::Mapping>> {
        match yaml {
            None => Some(None),
            Some(s) if s.trim().is_empty() => Some(Some(serde_yaml::Mapping::new())),
            Some(s) => match serde_yaml::from_str::<serde_yaml::Value>(s) {
                Ok(serde_yaml::Value::Mapping(m)) => Some(Some(m)),
                Ok(serde_yaml::Value::Null) => Some(Some(serde_yaml::Mapping::new())),
                _ => None,
            },
        }
    };

    let (base_m, ours_m, theirs_m) = match (parse(base_yaml), parse(ours_yaml), parse(theirs_yaml))
    {
        (Some(b), Some(o), Some(t)) => (b, o, t),
        _ => {
            // Conservative whole-block fallback: operator wins if it changed
            // the block; otherwise keep the agent block, else operator, else base.
            let operator_changed = base_yaml != theirs_yaml;
            let chosen = if operator_changed {
                theirs_yaml
            } else {
                ours_yaml.or(theirs_yaml)
            };
            return chosen.or(base_yaml).map(wrap);
        }
    };

    let base_m = base_m.unwrap_or_default();
    // A side with no frontmatter at all is treated as "unchanged from base" so it
    // cannot spuriously delete keys the other side authored.
    let ours_m = ours_m.unwrap_or_else(|| base_m.clone());
    let theirs_m = theirs_m.unwrap_or_else(|| base_m.clone());

    let key_str = |k: &serde_yaml::Value| k.as_str().map(|s| s.to_string());

    // Key order: operator first (operator owns layout), then agent-only, then base-only.
    let mut order: Vec<serde_yaml::Value> = Vec::new();
    let push_unique = |order: &mut Vec<serde_yaml::Value>, k: &serde_yaml::Value| {
        if !order.iter().any(|o| o == k) {
            order.push(k.clone());
        }
    };
    for k in theirs_m.keys() {
        push_unique(&mut order, k);
    }
    for k in ours_m.keys() {
        push_unique(&mut order, k);
    }
    for k in base_m.keys() {
        push_unique(&mut order, k);
    }

    let mut merged = serde_yaml::Mapping::new();
    for key in &order {
        let b = base_m.get(key);
        let o = ours_m.get(key);
        let t = theirs_m.get(key);

        let agent_changed = o != b;
        let operator_changed = t != b;

        let chosen: Option<&serde_yaml::Value> = if !agent_changed && !operator_changed {
            b.or(o).or(t)
        } else if agent_changed && operator_changed {
            if o == t {
                t
            } else if key_str(key)
                .as_deref()
                .is_some_and(is_agent_managed_frontmatter_key)
            {
                o
            } else {
                t
            }
        } else if operator_changed {
            t
        } else {
            o
        };

        if let Some(v) = chosen {
            merged.insert(key.clone(), v.clone());
        }
    }

    if merged.is_empty() {
        return Some("---\n---\n".to_string());
    }
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(merged)).ok()?;
    Some(wrap(&yaml))
}

/// Apply required-SSH defaults to a frontmatter using a pre-resolved context.
///
/// Pure: no `&Path`, no `std::fs`. The caller is responsible for resolving
/// the project config and computing the document's project-relative path
/// before invoking.
pub fn apply_required_ssh_defaults(
    fm: &mut Frontmatter,
    ctx: &SshResolverContext<'_>,
) -> Result<()> {
    if !fm.required_ssh_targets.is_empty() {
        fm.required_ssh_targets = normalize_required_ssh_targets(&fm.required_ssh_targets);
        return Ok(());
    }

    if ctx.doc_relative.is_empty() {
        return Ok(());
    }

    let resolved = if let Some(profile_name) = fm.required_ssh_profile.as_deref() {
        resolve_required_ssh_profile_targets(&ctx.project.ssh, profile_name, ctx.file_display)?
    } else if let Some(doc_cfg) = ctx.project.ssh.docs.get(ctx.doc_relative) {
        let mut targets = doc_cfg.targets.clone();
        if let Some(profile_name) = doc_cfg.profile.as_deref() {
            targets.extend(resolve_required_ssh_profile_targets(
                &ctx.project.ssh,
                profile_name,
                ctx.file_display,
            )?);
        }
        if targets.is_empty() {
            anyhow::bail!(
                "document {} is configured as SSH-dependent in .agent-doc/config.toml but no required_ssh_targets resolved for `{}`.\n\nAdd direct targets under [ssh.docs.\"{}\"] or define a valid profile under [ssh.profiles.<name>].",
                ctx.file_display,
                ctx.doc_relative,
                ctx.doc_relative
            );
        }
        targets
    } else {
        Vec::new()
    };

    fm.required_ssh_targets = normalize_required_ssh_targets(&resolved);
    Ok(())
}

fn resolve_required_ssh_profile_targets(
    ssh: &crate::project_config::SshConfig,
    profile_name: &str,
    file_display: &str,
) -> Result<Vec<String>> {
    let profile = ssh.profiles.get(profile_name).ok_or_else(|| {
        anyhow::anyhow!(
            "document {} requires SSH profile `{}` but .agent-doc/config.toml has no matching [ssh.profiles.{}] entry.",
            file_display,
            profile_name,
            profile_name
        )
    })?;
    if profile.targets.is_empty() {
        anyhow::bail!(
            "document {} requires SSH profile `{}` but [ssh.profiles.{}] has no targets.",
            file_display,
            profile_name,
            profile_name
        );
    }
    Ok(profile.targets.clone())
}

fn normalize_required_ssh_targets(targets: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for target in targets {
        let trimmed = target.trim();
        if trimmed.is_empty() || normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn contextualize_yaml_parse_error(
    file_display: &str,
    yaml: &str,
    err: serde_yaml::Error,
) -> anyhow::Error {
    let excerpt = err
        .location()
        .and_then(|location| render_frontmatter_excerpt(yaml, location.line(), location.column()));
    let mut message = format!("invalid YAML frontmatter in {}: {}", file_display, err);
    if let Some(excerpt) = excerpt {
        message.push_str("\n\nFrontmatter excerpt:\n");
        message.push_str(&excerpt);
    }
    message.push_str(
        "\n\nFix the frontmatter between the opening and closing --- markers, then rerun the command.",
    );
    anyhow::anyhow!(message)
}

fn render_frontmatter_excerpt(yaml: &str, line: usize, column: usize) -> Option<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    if line == 0 || line > lines.len() {
        return None;
    }

    let start = line.saturating_sub(1).max(1);
    let end = (line + 1).min(lines.len());
    let width = end.to_string().len();
    let mut rendered = Vec::new();

    for current in start..=end {
        let marker = if current == line { '>' } else { ' ' };
        rendered.push(format!(
            "{} {:>width$} | {}",
            marker,
            current,
            lines[current - 1],
            width = width
        ));
        if current == line {
            rendered.push(format!(
                "  {:>width$} | {}^",
                "",
                " ".repeat(column.saturating_sub(1)),
                width = width
            ));
        }
    }

    Some(rendered.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- #fmreset: field-level frontmatter 3-way merge ----

    fn yaml_of(merged: &str) -> serde_yaml::Mapping {
        let (y, _) = split_frontmatter_parts(merged);
        serde_yaml::from_str(y.unwrap()).unwrap()
    }

    #[test]
    fn fm3way_operator_only_edit_wins_not_reverted() {
        let base = "agent_doc_format: template\nclaude_model: sonnet";
        let ours = base; // agent untouched
        let theirs = "agent_doc_format: template\nclaude_model: opus"; // operator edit
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["claude_model"].as_str(), Some("opus"));
    }

    #[test]
    fn fm3way_agent_only_managed_write_applies() {
        let base = "agent_doc_session: sid\nclaude_model: sonnet";
        let ours = "agent_doc_session: sid\nresume: conv-1\nclaude_model: sonnet";
        let theirs = base; // operator untouched
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["resume"].as_str(), Some("conv-1"));
        assert_eq!(m["claude_model"].as_str(), Some("sonnet"));
    }

    #[test]
    fn fm3way_concurrent_operator_edit_and_agent_managed_write_field_wise() {
        let base = "agent_doc_session: sid\nclaude_model: sonnet";
        let ours = "agent_doc_session: sid\nresume: conv-1\nclaude_model: sonnet"; // agent wrote resume
        let theirs = "agent_doc_session: sid\nclaude_model: opus"; // operator edited model
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["resume"].as_str(), Some("conv-1"), "agent write kept");
        assert_eq!(
            m["claude_model"].as_str(),
            Some("opus"),
            "operator edit kept"
        );
    }

    #[test]
    fn fm3way_same_key_conflict_operator_wins_for_operator_key() {
        // Both changed claude_model differently → operator (theirs) wins.
        let base = "claude_model: sonnet";
        let ours = "claude_model: haiku"; // agent
        let theirs = "claude_model: opus"; // operator
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["claude_model"].as_str(), Some("opus"));
    }

    #[test]
    fn fm3way_same_key_conflict_agent_wins_for_managed_key() {
        // Both changed resume differently → agent (ours) wins (managed key).
        let base = "resume: c0";
        let ours = "resume: agent-new"; // agent
        let theirs = "resume: operator-typo"; // operator (rare/unintended)
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["resume"].as_str(), Some("agent-new"));
    }

    #[test]
    fn fm3way_operator_adds_unknown_key_preserved() {
        let base = "agent_doc_format: template";
        let ours = base;
        let theirs = "agent_doc_format: template\nmy_custom_key: hello";
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert_eq!(m["my_custom_key"].as_str(), Some("hello"));
    }

    #[test]
    fn fm3way_operator_deletes_key() {
        let base = "agent_doc_format: template\nbranch: feature";
        let ours = base; // agent untouched
        let theirs = "agent_doc_format: template"; // operator removed branch
        let merged = merge_frontmatter_3way(Some(base), Some(ours), Some(theirs)).unwrap();
        let m = yaml_of(&merged);
        assert!(
            !m.contains_key("branch"),
            "operator deletion honored: {merged}"
        );
        assert!(m.contains_key("agent_doc_format"));
    }

    #[test]
    fn fm3way_none_when_no_frontmatter() {
        assert!(merge_frontmatter_3way(None, None, None).is_none());
    }

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Hello\n\nBody text.\n";
        let (fm, body) = parse(content).unwrap();
        assert!(fm.session.is_none());
        assert!(fm.agent.is_none());
        assert!(fm.model.is_none());
        assert!(fm.branch.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_tolerates_leading_horizontal_space_before_frontmatter() {
        let content = " ---\nagent_doc_session: abc-123\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn parse_tolerates_bom_before_frontmatter() {
        let content = "\u{feff}---\nagent_doc_session: abc-123\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn parse_keeps_leading_blank_line_as_body_not_frontmatter() {
        let content = "\n---\nagent_doc_session: abc-123\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert!(fm.session.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_all_fields() {
        let content =
            "---\nsession: abc-123\nagent: claude\nmodel: opus\nbranch: main\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.branch.as_deref(), Some("main"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn queue_control_parse_aliases() {
        assert_eq!(QueueControl::parse("start"), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("go"), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("STOP"), Some(QueueControl::Stop));
        assert_eq!(QueueControl::parse("  Start  "), Some(QueueControl::Start));
        assert_eq!(QueueControl::parse("auto"), None);
        assert_eq!(QueueControl::parse(""), None);
        assert!(QueueControl::Start.is_active());
        assert!(!QueueControl::Stop.is_active());
    }

    #[test]
    fn parse_queue_start_activates() {
        // `queue: start` (canonical) folds onto the deprecated queue_active bool
        // so every existing reader honors it (#queue-state-unify).
        let (fm, _) = parse("---\nagent_doc_format: template\nqueue: start\n---\n\n").unwrap();
        assert_eq!(fm.queue.as_deref(), Some("start"));
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_queue_go_alias_activates() {
        let (fm, _) = parse("---\nqueue: go\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_queue_stop_deactivates() {
        let (fm, _) = parse("---\nqueue: stop\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(false));
    }

    #[test]
    fn parse_queue_canonical_wins_over_stale_queue_active() {
        // An explicit `queue:` control overrides a stale `queue_active:` line.
        let (fm, _) = parse("---\nqueue: stop\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(false));

        let (fm, _) = parse("---\nqueue: start\nqueue_active: false\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn pipeline_is_empty_default() {
        let p = AgentDocPipeline::default();
        assert!(p.is_empty());
    }

    #[test]
    fn parse_pipeline_all_fields() {
        let content = "---\nagent_doc_pipeline:\n  turn_id: \"#fm-run-id-step\"\n  run_id: cycle-1780600334196\n  step: response_captured\n  queue_task_id: \"#fm-run-id-step\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.pipeline.turn_id.as_deref(), Some("#fm-run-id-step"));
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-1780600334196"));
        assert_eq!(fm.pipeline.step.as_deref(), Some("response_captured"));
        assert_eq!(
            fm.pipeline.queue_task_id.as_deref(),
            Some("#fm-run-id-step")
        );
        assert!(!fm.pipeline.is_empty());
    }

    #[test]
    fn empty_pipeline_is_not_serialized() {
        // A drained/terminal cycle must leave no `agent_doc_pipeline:` block behind.
        let fm = Frontmatter {
            session: Some("abc".to_string()),
            ..Default::default()
        };
        let out = write(&fm, "Body\n").unwrap();
        assert!(!out.contains("agent_doc_pipeline"));
    }

    #[test]
    fn set_pipeline_state_merges_without_disturbing_other_fields() {
        let content =
            "---\nagent_doc_session: abc-123\nagent: claude\nqueue: start\n---\n\n## Body\n";
        // Set run_id + step, leave turn_id/queue_task_id untouched.
        let updated =
            set_pipeline_state(content, Some("cycle-99"), Some("write_applied"), None, None)
                .unwrap();
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm.pipeline.step.as_deref(), Some("write_applied"));
        assert!(fm.pipeline.turn_id.is_none());
        // Other frontmatter fields and the body survive untouched.
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.queue_active, Some(true));
        assert!(body.contains("## Body"));

        // A second merge updates step + turn_id while preserving the prior run_id.
        let updated2 = set_pipeline_state(
            &updated,
            None,
            Some("committed"),
            Some("#fm-run-id-step"),
            None,
        )
        .unwrap();
        let (fm2, _) = parse(&updated2).unwrap();
        assert_eq!(fm2.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm2.pipeline.step.as_deref(), Some("committed"));
        assert_eq!(fm2.pipeline.turn_id.as_deref(), Some("#fm-run-id-step"));
    }

    #[test]
    fn splice_pipeline_block_is_byte_precise_and_strip_cancels() {
        // #22a8: the write-side splice must touch ONLY the pipeline block, and
        // the diff-side strip must cancel it back to the original bytes.
        let base = "---\nagent_doc_session: abc-123\nagent: claude\nprompt_presets:\n  '#x': do the thing\nqueue: start\n---\n\n## Body\n- item\n";
        let pipeline = AgentDocPipeline {
            run_id: Some("cycle-99".into()),
            step: Some("response_captured".into()),
            turn_id: Some("#22a8".into()),
            queue_task_id: None,
        };
        let spliced = splice_pipeline_block(base, &pipeline).unwrap();
        // The block is present and parseable…
        assert!(spliced.contains("agent_doc_pipeline:"));
        let (fm, _) = parse(&spliced).unwrap();
        assert_eq!(fm.pipeline.run_id.as_deref(), Some("cycle-99"));
        assert_eq!(fm.pipeline.turn_id.as_deref(), Some("#22a8"));
        // …every non-block field/byte is preserved (presets, order, body).
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        // Stripping the block yields the ORIGINAL bytes exactly.
        assert_eq!(strip_pipeline_block_lines(&spliced), base);
        // Clearing (empty pipeline) also returns the original bytes.
        assert_eq!(
            splice_pipeline_block(&spliced, &AgentDocPipeline::default()).unwrap(),
            base
        );
        // Idempotent: re-splicing the same state does not stack blocks.
        let twice = splice_pipeline_block(&spliced, &pipeline).unwrap();
        assert_eq!(twice.matches("agent_doc_pipeline:").count(), 1);
    }

    #[test]
    fn clear_pipeline_state_drops_block_only() {
        let content = "---\nagent_doc_session: abc-123\nagent_doc_pipeline:\n  run_id: cycle-1\n  step: response_captured\n---\n\nBody\n";
        let cleared = clear_pipeline_state(content).unwrap();
        assert!(!cleared.contains("agent_doc_pipeline"));
        let (fm, body) = parse(&cleared).unwrap();
        assert!(fm.pipeline.is_empty());
        assert_eq!(fm.session.as_deref(), Some("abc-123"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_queue_unknown_value_leaves_queue_active_untouched() {
        // A typo must never silently flip activation.
        let (fm, _) = parse("---\nqueue: bogus\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, Some(true));
        let (fm, _) = parse("---\nqueue: bogus\n---\n\n").unwrap();
        assert_eq!(fm.queue_active, None);
    }

    #[test]
    fn parse_free_text_execution_mode() {
        let (fm, _) = parse("---\nagent_doc_free_text_execution: queue\n---\n\n").unwrap();
        assert_eq!(fm.free_text_execution, Some(FreeTextExecutionMode::Queue));
        let (fm, _) = parse("---\nfree_text_execution: goal\n---\n\n").unwrap();
        assert_eq!(fm.free_text_execution, Some(FreeTextExecutionMode::Goal));
    }

    #[test]
    fn write_emits_canonical_queue_for_direct_set_queue_active() {
        // #writer-emits-deprecated-queue-active-false: a path that sets
        // `queue_active` directly on the struct (bypassing merge_queue_state) and
        // then calls write() must still emit the canonical `queue:` control, never
        // the deprecated `queue_active:` line.
        let mut fm = Frontmatter {
            queue_active: Some(true),
            ..Default::default()
        };
        let out = write(&fm, "body\n").unwrap();
        assert!(out.contains("queue: start"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        fm.queue_active = Some(false);
        let out = write(&fm, "body\n").unwrap();
        assert!(out.contains("queue: stop"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        // A doc parsed from canonical `queue:` (queue_active mirrored on) must
        // not double-emit the deprecated line on re-serialize.
        let (fm, body) =
            parse("---\nagent_doc_format: template\nqueue: start\n---\n\nx\n").unwrap();
        let out = write(&fm, &body).unwrap();
        assert!(out.contains("queue: start"), "{out}");
        assert!(!out.contains("queue_active:"), "{out}");

        // No queue state → neither key is emitted.
        let fm = Frontmatter::default();
        let out = write(&fm, "body\n").unwrap();
        assert!(
            !out.contains("queue_active:") && !out.contains("queue:"),
            "{out}"
        );
    }

    #[test]
    fn merge_queue_state_writes_canonical_and_drops_legacy() {
        // #queue-state-unify phase 4: writer emits `queue: start|stop` and
        // removes any legacy `queue_active:` line so `queue:` is the sole field.
        let legacy = "---\nagent_doc_format: template\nqueue_active: true\n---\n\nbody\n";
        let active = merge_queue_state(legacy, true).unwrap();
        assert!(active.contains("queue: start"), "{active}");
        assert!(!active.contains("queue_active:"), "{active}");

        let stopped = merge_queue_state(legacy, false).unwrap();
        assert!(stopped.contains("queue: stop"), "{stopped}");
        assert!(!stopped.contains("queue_active:"), "{stopped}");

        // Round-trips back to the internal queue_active for readers.
        let (fm, _) = parse(&active).unwrap();
        assert_eq!(fm.queue_active, Some(true));
        let (fm, _) = parse(&stopped).unwrap();
        assert_eq!(fm.queue_active, Some(false));
    }

    #[test]
    fn strip_deprecated_queue_active_line_drops_legacy_when_canonical_present() {
        // #queue-active-deprecated-line-stuck: a doc carrying BOTH `queue:` and the
        // deprecated `queue_active:` (the stuck shape the diff layer never lets a
        // normal write remove) has the legacy line dropped byte-precisely, leaving
        // every other field untouched.
        let stuck = "---\nagent: claude\nqueue: start\nqueue_active: true\n---\n\nbody\n";
        let out = strip_deprecated_queue_active_line(stuck);
        assert_eq!(
            out, "---\nagent: claude\nqueue: start\n---\n\nbody\n",
            "legacy line dropped, canonical queue + other fields preserved"
        );
        // Idempotent.
        assert_eq!(strip_deprecated_queue_active_line(&out), out);
    }

    #[test]
    fn strip_deprecated_queue_active_line_keeps_legacy_without_canonical() {
        // Without a canonical `queue:` the legacy line is the only state — never
        // drop it (that would lose the queue state).
        let legacy_only = "---\nqueue_active: true\n---\n\nbody\n";
        assert_eq!(strip_deprecated_queue_active_line(legacy_only), legacy_only);
        // No frontmatter at all → unchanged.
        assert_eq!(
            strip_deprecated_queue_active_line("no frontmatter\nqueue_active: true\n"),
            "no frontmatter\nqueue_active: true\n"
        );
    }

    #[test]
    fn parse_legacy_queue_active_still_honored() {
        // Back-compat: docs without the canonical key keep working.
        let (fm, _) = parse("---\nqueue_active: true\n---\n\n").unwrap();
        assert_eq!(fm.queue.as_deref(), None);
        assert_eq!(fm.queue_active, Some(true));
    }

    #[test]
    fn parse_partial_fields() {
        let content = "---\nsession: xyz\n---\n# Doc\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("xyz"));
        assert!(fm.agent.is_none());
        assert!(body.contains("# Doc"));
    }

    #[test]
    fn parse_model_tier_high() {
        let content = "---\nagent_doc_model_tier: high\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::High));
    }

    #[test]
    fn parse_collaboration_mode_shared() {
        let content = "---\nagent_doc_collaboration: shared\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.collaboration_mode(), CollaborationMode::Shared);
    }

    #[test]
    fn parse_security_review_roundtrip() {
        let content = "---\nagent_doc_collaboration: shared\nagent_doc_security_review: sec-2026-04-29\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.collaboration_mode(), CollaborationMode::Shared);
        assert_eq!(fm.security_review.as_deref(), Some("sec-2026-04-29"));
        assert!(fm.has_security_review());
        let written = write(&fm, "Body\n").unwrap();
        assert!(written.contains("agent_doc_collaboration: shared"));
        assert!(written.contains("agent_doc_security_review: sec-2026-04-29"));
    }

    #[test]
    fn parse_model_tier_low() {
        let content = "---\nagent_doc_model_tier: low\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Low));
    }

    #[test]
    fn parse_model_tier_med() {
        let content = "---\nagent_doc_model_tier: med\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Med));
    }

    #[test]
    fn parse_model_tier_auto() {
        let content = "---\nagent_doc_model_tier: auto\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, Some(Tier::Auto));
    }

    #[test]
    fn parse_model_tier_absent() {
        let content = "---\nagent: claude\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model_tier, None);
    }

    #[test]
    fn parse_pending_capture_guard_strict() {
        let content = "---\npending_capture_guard: strict\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(
            fm.pending_capture_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_pending_capture_guard_invalid_rejected() {
        let content = "---\npending_capture_guard: loudly\n---\nBody\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn parse_pending_done_guard_strict() {
        let content = "---\npending_done_guard: strict\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.pending_done_guard, Some(PendingCaptureGuardMode::Strict));
    }

    #[test]
    fn parse_pending_done_guard_invalid_rejected() {
        let content = "---\npending_done_guard: loudly\n---\nBody\n";
        assert!(parse(content).is_err());
    }

    #[test]
    fn parse_model_tier_invalid_rejected() {
        let content = "---\nagent_doc_model_tier: ultra\n---\nBody\n";
        let result = parse(content);
        assert!(result.is_err(), "invalid tier value should fail to parse");
    }

    #[test]
    fn write_model_tier_roundtrip() {
        let fm = Frontmatter {
            model_tier: Some(Tier::High),
            ..Default::default()
        };
        let doc = write(&fm, "Body\n").unwrap();
        let (parsed, _) = parse(&doc).unwrap();
        assert_eq!(parsed.model_tier, Some(Tier::High));
        assert!(doc.contains("agent_doc_model_tier: high"));
    }

    #[test]
    fn parse_null_fields() {
        let content = "---\nsession: null\nagent: null\nmodel: null\nbranch: null\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert!(fm.session.is_none());
        assert!(fm.agent.is_none());
        assert!(fm.model.is_none());
        assert!(fm.branch.is_none());
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_unterminated_frontmatter() {
        let content = "---\nsession: abc\nno closing block";
        let err = parse(content).unwrap_err();
        assert!(err.to_string().contains("Unterminated frontmatter"));
    }

    #[test]
    fn parse_closing_at_eof() {
        let content = "---\nsession: abc\n---";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(body, "");
    }

    #[test]
    fn parse_empty_body() {
        let content = "---\nsession: abc\n---\n";
        let (fm, _body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
    }

    #[test]
    fn write_roundtrip() {
        // Start from write output to ensure consistent formatting
        let fm = Frontmatter {
            session: Some("test-id".to_string()),
            resume: Some("resume-id".to_string()),
            agent: Some("claude".to_string()),
            model: Some("opus".to_string()),
            claude_model: None,
            codex_model: None,
            opencode_model: None,
            branch: Some("dev".to_string()),
            tmux_session: None,
            mode: None,
            format: None,
            write_mode: None,
            stream_config: None,
            agent_args: None,
            claude_args: None,
            codex_args: None,
            opencode_args: None,
            codex_network_access: None,
            required_ssh_targets: Vec::new(),
            required_ssh_profile: None,
            managed_proof_max_attempts: None,
            managed_proof_retry_backoff_secs: None,
            managed_proof_probe_timeout_secs: None,
            no_mcp: None,
            enable_tool_search: None,
            debounce_ms: None,
            links: vec![],
            auto_compact: None,
            queue_context_reset: None,
            gate_autoverify: None,
            clear_threshold: None,
            supervisor_auto_recycle: None,
            agent_change_restart: None,
            supervisor_auto_install: None,
            model_tier: None,
            pending_capture_guard: None,
            pending_done_guard: None,
            review_done_guard: None,
            auto_done: None,
            agent_doc_lint_dialect: None,
            hooks: std::collections::HashMap::new(),
            env: indexmap::IndexMap::new(),
            prompt_presets: indexmap::IndexMap::new(),
            free_text_execution: None,
            dispatch: None,
            agent_doc_env_inherit: None,
            cwd: None,
            queue: None,
            queue_active: None,
            queue_prompt_echo_max_chars: None,
            collaboration: None,
            security_review: None,
            pipeline: AgentDocPipeline::default(),
        };
        let body = "# Hello\n\nBody text.\n";
        let written = write(&fm, body).unwrap();
        let (fm2, body2) = parse(&written).unwrap();
        assert_eq!(fm2.session, fm.session);
        assert_eq!(fm2.agent, fm.agent);
        assert_eq!(fm2.model, fm.model);
        assert_eq!(fm2.claude_model, fm.claude_model);
        assert_eq!(fm2.codex_model, fm.codex_model);
        assert_eq!(fm2.opencode_model, fm.opencode_model);
        assert_eq!(fm2.branch, fm.branch);
        assert_eq!(fm2.collaboration, fm.collaboration);
        assert_eq!(fm2.security_review, fm.security_review);
        assert_eq!(body2, body);
    }

    #[test]
    fn repeated_parse_write_roundtrip_preserves_body_prefix() {
        let fm = Frontmatter {
            session: Some("abc".to_string()),
            agent: Some("codex".to_string()),
            ..Default::default()
        };
        let original = write(&fm, "## Status\n\nBody\n").unwrap();
        let mut current = original.clone();

        for _ in 0..3 {
            let (fm, body) = parse(&current).unwrap();
            assert_eq!(body, "## Status\n\nBody\n");
            current = write(&fm, body).unwrap();
        }

        let (fm, body) = parse(&current).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(fm.agent.as_deref(), Some("codex"));
        assert_eq!(body, "## Status\n\nBody\n");
        assert_eq!(current, original);
    }

    #[test]
    fn write_default_frontmatter() {
        let fm = Frontmatter::default();
        let result = write(&fm, "body\n").unwrap();
        assert!(result.starts_with("---\n"));
        assert!(result.ends_with("---\nbody\n"));
    }

    #[test]
    fn write_preserves_body_content() {
        let fm = Frontmatter::default();
        let body = "# Title\n\nSome **markdown** with `code`.\n";
        let result = write(&fm, body).unwrap();
        assert!(result.contains("# Title"));
        assert!(result.contains("Some **markdown** with `code`."));
    }

    #[test]
    fn set_session_id_creates_frontmatter() {
        let content = "# No frontmatter\n\nJust body.\n";
        let result = set_session_id(content, "new-session").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new-session"));
        assert!(body.contains("# No frontmatter"));
    }

    #[test]
    fn set_session_id_updates_existing() {
        let content = "---\nsession: old-id\nagent: claude\n---\nBody\n";
        let result = set_session_id(content, "new-id").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new-id"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn set_session_id_preserves_other_fields() {
        let content = "---\nsession: old\nagent: claude\nmodel: opus\nbranch: dev\n---\nBody\n";
        let result = set_session_id(content, "new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("new"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.branch.as_deref(), Some("dev"));
    }

    #[test]
    fn ensure_session_no_frontmatter() {
        let content = "# Hello\n\nBody.\n";
        let (updated, sid) = ensure_session(content).unwrap();
        // Should have generated a UUID
        assert_eq!(sid.len(), 36); // UUID v4 string length
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.session.as_deref(), Some(sid.as_str()));
        assert!(body.contains("# Hello"));
    }

    #[test]
    fn ensure_session_null_session() {
        let content = "---\nsession:\nagent: claude\n---\nBody\n";
        let (updated, sid) = ensure_session(content).unwrap();
        assert_eq!(sid.len(), 36);
        let (fm, body) = parse(&updated).unwrap();
        assert_eq!(fm.session.as_deref(), Some(sid.as_str()));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn ensure_session_existing_session() {
        let content = "---\nagent_doc_session: existing-id\nagent: claude\n---\nBody\n";
        let (updated, sid) = ensure_session(content).unwrap();
        assert_eq!(sid, "existing-id");
        // Content should be unchanged
        assert_eq!(updated, content);
    }

    #[test]
    fn parse_legacy_session_field() {
        // Old `session:` field should still parse via serde alias
        let content = "---\nsession: legacy-id\nagent: claude\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(fm.session.as_deref(), Some("legacy-id"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn parse_agent_doc_mode_canonical() {
        let content = "---\nagent_doc_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn parse_mode_shorthand_alias() {
        let content = "---\nmode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn parse_response_mode_legacy_alias() {
        let content = "---\nresponse_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.mode.as_deref(), Some("template"));
    }

    #[test]
    fn write_uses_agent_doc_mode_field() {
        #[allow(deprecated)]
        let fm = Frontmatter {
            mode: Some("template".to_string()),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_mode:"));
        assert!(!result.contains("response_mode:"));
        assert!(!result.contains("\nmode:"));
    }

    #[test]
    fn write_uses_new_field_name() {
        let fm = Frontmatter {
            session: Some("test-id".to_string()),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_session:"));
        assert!(!result.contains("\nsession:"));
    }

    #[test]
    fn write_pending_capture_guard_roundtrip() {
        let fm = Frontmatter {
            pending_capture_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("pending_capture_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.pending_capture_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn write_pending_done_guard_roundtrip() {
        let fm = Frontmatter {
            pending_done_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("pending_done_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.pending_done_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_review_done_guard_error_alias() {
        let content = "---\nreview_done_guard: error\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.review_done_guard, Some(PendingCaptureGuardMode::Strict));
    }

    #[test]
    fn write_review_done_guard_roundtrip() {
        let fm = Frontmatter {
            review_done_guard: Some(PendingCaptureGuardMode::Strict),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        assert!(written.contains("review_done_guard: strict"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(
            parsed.review_done_guard,
            Some(PendingCaptureGuardMode::Strict)
        );
    }

    #[test]
    fn parse_auto_done_bool() {
        let content = "---\nauto_done: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.auto_done, Some(true));
    }

    #[test]
    fn write_auto_done_roundtrip() {
        let fm = Frontmatter {
            auto_done: Some(true),
            ..Default::default()
        };
        let written = write(&fm, "Body\n").unwrap();
        assert!(written.contains("auto_done: true"));
        let (parsed, _) = parse(&written).unwrap();
        assert_eq!(parsed.auto_done, Some(true));
    }

    #[test]
    fn parse_prompt_presets_roundtrip() {
        let content = "---\nprompt_presets:\n  \"#1\": |\n    Today is 2026-04-25.\n    Keep the work tree clean.\n  release-check: |\n    Run cargo test.\n---\nBody\n";
        let (fm, body) = parse(content).unwrap();
        assert_eq!(body, "Body\n");
        assert_eq!(
            fm.prompt_presets.get("#1").map(String::as_str),
            Some("Today is 2026-04-25.\nKeep the work tree clean.\n")
        );
        assert_eq!(
            fm.prompt_presets.get("release-check").map(String::as_str),
            Some("Run cargo test.")
        );

        let written = write(&fm, body).unwrap();
        let (parsed, body2) = parse(&written).unwrap();
        assert_eq!(body2, "Body\n");
        assert_eq!(parsed.prompt_presets, fm.prompt_presets);
    }

    #[test]
    fn resolve_prompt_preset_key_accepts_bare_hashtag_alias() {
        let mut prompt_presets = indexmap::IndexMap::new();
        prompt_presets.insert("#spec-test".to_string(), "Run checks.".to_string());
        prompt_presets.insert("release-check".to_string(), "Publish.".to_string());

        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "spec-test").as_deref(),
            Some("#spec-test")
        );
        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "#spec-test").as_deref(),
            Some("#spec-test")
        );
        assert_eq!(
            resolve_prompt_preset_key(&prompt_presets, "release-check").as_deref(),
            Some("release-check")
        );
        assert!(resolve_prompt_preset_key(&prompt_presets, "missing").is_none());
    }

    // --- resolve_mode tests ---

    #[test]
    fn resolve_mode_defaults() {
        let fm = Frontmatter::default();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_append() {
        let content = "---\nagent_doc_mode: append\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_template() {
        let content = "---\nagent_doc_mode: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_from_deprecated_stream() {
        let content = "---\nagent_doc_mode: stream\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_new_fields_override_deprecated() {
        let content = "---\nagent_doc_mode: append\nagent_doc_format: template\nagent_doc_write: merge\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template);
        assert_eq!(resolved.write, AgentDocWrite::Merge);
    }

    #[test]
    fn resolve_mode_explicit_new_fields_only() {
        let content = "---\nagent_doc_format: append\nagent_doc_write: crdt\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt);
    }

    #[test]
    fn resolve_mode_partial_new_field_format_only() {
        let content = "---\nagent_doc_format: append\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Append);
        assert_eq!(resolved.write, AgentDocWrite::Crdt); // default
    }

    #[test]
    fn resolve_mode_partial_new_field_write_only() {
        let content = "---\nagent_doc_write: merge\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        let resolved = fm.resolve_mode();
        assert_eq!(resolved.format, AgentDocFormat::Template); // default
        assert_eq!(resolved.write, AgentDocWrite::Merge);
    }

    #[test]
    fn resolve_mode_helper_methods() {
        let fm = Frontmatter::default();
        let resolved = fm.resolve_mode();
        assert!(resolved.is_template());
        assert!(!resolved.is_append());
        assert!(resolved.is_crdt());
    }

    #[test]
    fn parse_new_format_field() {
        let content = "---\nagent_doc_format: template\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
    }

    #[test]
    fn parse_new_write_field() {
        let content = "---\nagent_doc_write: crdt\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    #[test]
    fn write_uses_new_format_write_fields() {
        let fm = Frontmatter {
            format: Some(AgentDocFormat::Template),
            write_mode: Some(AgentDocWrite::Crdt),
            ..Default::default()
        };
        let result = write(&fm, "body\n").unwrap();
        assert!(result.contains("agent_doc_format:"));
        assert!(result.contains("agent_doc_write:"));
        assert!(!result.contains("agent_doc_mode:"));
    }

    #[test]
    fn set_format_and_write_clears_deprecated_mode() {
        let content = "---\nagent_doc_mode: stream\n---\nBody\n";
        let result =
            set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    // --- merge_fields tests ---

    #[test]
    fn merge_fields_adds_new_field() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "model: opus").unwrap();
        let (fm, body) = parse(&result).unwrap();
        assert_eq!(fm.session.as_deref(), Some("abc"));
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert!(body.contains("Body"));
    }

    #[test]
    fn merge_fields_updates_existing_field() {
        let content = "---\nagent_doc_session: abc\nmodel: sonnet\n---\nBody\n";
        let result = merge_fields(content, "model: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.session.as_deref(), Some("abc"));
    }

    #[test]
    fn merge_fields_multiple_fields() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "model: opus\nagent: claude\nbranch: main").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
        assert_eq!(fm.agent.as_deref(), Some("claude"));
        assert_eq!(fm.branch.as_deref(), Some("main"));
    }

    #[test]
    fn merge_fields_format_enum() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "agent_doc_format: append").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.format, Some(AgentDocFormat::Append));
    }

    #[test]
    fn merge_fields_write_enum() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "agent_doc_write: merge").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Merge));
    }

    #[test]
    fn merge_fields_ignores_unknown() {
        let content = "---\nagent_doc_session: abc\n---\nBody\n";
        let result = merge_fields(content, "unknown_field: value\nmodel: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("opus"));
    }

    #[test]
    fn merge_fields_preserves_body() {
        let content = "---\nagent_doc_session: abc\n---\n# Title\n\nSome **markdown** content.\n";
        let result = merge_fields(content, "model: opus").unwrap();
        assert!(result.contains("# Title"));
        assert!(result.contains("Some **markdown** content."));
    }

    #[test]
    fn set_format_and_write_clears_deprecated() {
        let content = "---\nagent_doc_mode: append\n---\nBody\n";
        let result =
            set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    #[test]
    fn hooks_roundtrip() {
        let content = "---\nhooks:\n  session_start:\n    - \"echo start {{session_id}}\"\n  post_write:\n    - \"notify {{file}}\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(
            fm.hooks.get("session_start"),
            Some(&vec!["echo start {{session_id}}".to_string()])
        );
        assert_eq!(
            fm.hooks.get("post_write"),
            Some(&vec!["notify {{file}}".to_string()])
        );
    }

    #[test]
    fn hooks_omitted_when_empty() {
        let fm = Frontmatter::default();
        let result = write(&fm, "body\n").unwrap();
        assert!(!result.contains("hooks"));
    }

    #[test]
    fn hooks_absent_parses_as_empty() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.hooks.is_empty());
    }

    #[test]
    fn parse_no_mcp_field() {
        let content = "---\nno_mcp: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.no_mcp, Some(true));
    }

    #[test]
    fn parse_enable_tool_search_field() {
        let content = "---\nenable_tool_search: true\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.enable_tool_search, Some(true));
    }

    #[test]
    fn parse_missing_flags_default_none() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.no_mcp.is_none());
        assert!(fm.enable_tool_search.is_none());
    }

    #[test]
    fn parse_env_map() {
        let content = "---\nenv:\n  FOO: bar\n  BAZ: \"$(echo hello)\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.env.len(), 2);
        assert_eq!(fm.env["FOO"], Some("bar".to_string()));
        assert_eq!(fm.env["BAZ"], Some("$(echo hello)".to_string()));
        // Verify order is preserved
        let keys: Vec<&String> = fm.env.keys().collect();
        assert_eq!(keys, vec!["FOO", "BAZ"]);
    }

    #[test]
    fn parse_env_empty_default() {
        let content = "---\nsession: abc\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert!(fm.env.is_empty());
    }

    #[test]
    fn parse_env_unset_via_null() {
        let content = "---\nenv:\n  SET_ME: value\n  UNSET_ME: null\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.env.len(), 2);
        assert_eq!(fm.env["SET_ME"], Some("value".to_string()));
        assert_eq!(fm.env["UNSET_ME"], None);
    }

    #[test]
    fn write_roundtrip_with_env() {
        let mut env: indexmap::IndexMap<String, Option<String>> = indexmap::IndexMap::new();
        env.insert("KEY1".to_string(), Some("value1".to_string()));
        env.insert("KEY2".to_string(), Some("$KEY1".to_string()));
        env.insert("KEY3".to_string(), None);
        let fm = Frontmatter {
            env,
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        let (fm2, _) = parse(&written).unwrap();
        assert_eq!(fm2.env.len(), 3);
        assert_eq!(fm2.env["KEY1"], Some("value1".to_string()));
        assert_eq!(fm2.env["KEY2"], Some("$KEY1".to_string()));
        assert_eq!(fm2.env["KEY3"], None);
    }

    #[test]
    fn parse_agent_args_field() {
        let content = "---\nagent_args: \"--json -s workspace-write\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert!(fm.claude_args.is_none());
        assert!(fm.codex_args.is_none());
        assert!(fm.opencode_args.is_none());
    }

    #[test]
    fn parse_agent_args_and_harness_aliases() {
        let content = "---\nagent_args: \"--json\"\nclaude_args: \"--dangerously-skip-permissions\"\ncodex_args: \"-s danger-full-access\"\nopencode_args: \"--dangerously-skip-permissions\"\ncodex_network_access: enabled\nrequired_ssh_targets:\n  - sampleorders-server\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("--json"));
        assert_eq!(
            fm.claude_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(
            fm.opencode_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm.codex_network_access, Some(CodexNetworkAccess::Enabled));
        assert_eq!(
            fm.required_ssh_targets,
            vec!["sampleorders-server".to_string()]
        );
    }

    #[test]
    fn write_roundtrip_agent_args() {
        let fm = Frontmatter {
            agent_args: Some("--json -s workspace-write".to_string()),
            codex_args: Some("-s danger-full-access".to_string()),
            opencode_args: Some("--dangerously-skip-permissions".to_string()),
            codex_network_access: Some(CodexNetworkAccess::Enabled),
            required_ssh_targets: vec!["sampleorders-server".to_string()],
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        let (fm2, _) = parse(&written).unwrap();
        assert_eq!(fm2.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert_eq!(fm2.codex_args.as_deref(), Some("-s danger-full-access"));
        assert_eq!(
            fm2.opencode_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm2.codex_network_access, Some(CodexNetworkAccess::Enabled));
        assert_eq!(
            fm2.required_ssh_targets,
            vec!["sampleorders-server".to_string()]
        );
    }

    #[test]
    fn merge_fields_agent_args() {
        let content = "---\nclaude_args: old\ncodex_args: old-codex\nopencode_args: old-opencode\n---\nBody\n";
        let result = merge_fields(content, "agent_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("new"));
        assert_eq!(fm.claude_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("old-codex"));
        assert_eq!(fm.opencode_args.as_deref(), Some("old-opencode"));
    }

    #[test]
    fn merge_fields_codex_args() {
        let content = "---\nagent_args: old\n---\nBody\n";
        let result = merge_fields(content, "codex_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("new"));
    }

    #[test]
    fn merge_fields_opencode_args() {
        let content = "---\nagent_args: old\n---\nBody\n";
        let result = merge_fields(content, "opencode_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("old"));
        assert_eq!(fm.opencode_args.as_deref(), Some("new"));
    }

    #[test]
    fn merge_fields_codex_network_access() {
        let content = "Body\n";
        let result = merge_fields(content, "codex_network_access: disabled").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.codex_network_access, Some(CodexNetworkAccess::Disabled));
    }

    #[test]
    fn merge_fields_required_ssh_targets() {
        let content = "Body\n";
        let result = merge_fields(
            content,
            "required_ssh_targets:\n  - sampleorders-server\n  - root@50.28.2.199",
        )
        .unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(
            fm.required_ssh_targets,
            vec![
                "sampleorders-server".to_string(),
                "root@50.28.2.199".to_string(),
            ]
        );
    }

    #[test]
    fn merge_fields_required_ssh_profile() {
        let content = "Body\n";
        let result = merge_fields(content, "required_ssh_profile: sampleorders").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.required_ssh_profile.as_deref(), Some("sampleorders"));
    }

    // --- resolve_harness_model tests ---

    #[test]
    fn resolve_harness_model_claude_code_uses_claude_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("claude-code"), Some("opus"));
    }

    #[test]
    fn resolve_harness_model_codex_uses_codex_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("codex"), Some("o3-pro"));
    }

    #[test]
    fn resolve_harness_model_opencode_uses_opencode_model() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("opencode"), Some("zai/glm-5"));
    }

    #[test]
    fn resolve_harness_model_falls_back_to_generic() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("claude-code"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("codex"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("opencode"), Some("gpt-5"));
        assert_eq!(fm.resolve_harness_model("default"), Some("gpt-5"));
    }

    #[test]
    fn resolve_harness_model_none_when_no_model() {
        let fm = Frontmatter::default();
        assert_eq!(fm.resolve_harness_model("claude-code"), None);
    }

    #[test]
    fn resolve_harness_model_unknown_harness_uses_generic() {
        let fm = Frontmatter {
            model: Some("sonnet".to_string()),
            claude_model: Some("opus".to_string()),
            ..Default::default()
        };
        assert_eq!(fm.resolve_harness_model("unknown"), Some("sonnet"));
    }

    #[test]
    fn parse_harness_model_fields() {
        let content = "---\nmodel: gpt-5\nclaude_model: opus\ncodex_model: o3-pro\nopencode_model: zai/glm-5\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.claude_model.as_deref(), Some("opus"));
        assert_eq!(fm.codex_model.as_deref(), Some("o3-pro"));
        assert_eq!(fm.opencode_model.as_deref(), Some("zai/glm-5"));
    }

    #[test]
    fn write_roundtrip_harness_model_fields() {
        let fm = Frontmatter {
            model: Some("gpt-5".to_string()),
            claude_model: Some("opus".to_string()),
            codex_model: Some("o3-pro".to_string()),
            opencode_model: Some("zai/glm-5".to_string()),
            ..Default::default()
        };
        let doc = write(&fm, "Body\n").unwrap();
        let (fm2, body) = parse(&doc).unwrap();
        assert_eq!(fm2.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm2.claude_model.as_deref(), Some("opus"));
        assert_eq!(fm2.codex_model.as_deref(), Some("o3-pro"));
        assert_eq!(fm2.opencode_model.as_deref(), Some("zai/glm-5"));
        assert_eq!(body, "Body\n");
    }

    #[test]
    fn merge_fields_claude_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "claude_model: opus").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.claude_model.as_deref(), Some("opus"));
    }

    #[test]
    fn merge_fields_codex_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "codex_model: o3-pro").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.codex_model.as_deref(), Some("o3-pro"));
    }

    #[test]
    fn merge_fields_opencode_model() {
        let content = "---\nmodel: gpt-5\n---\nBody\n";
        let result = merge_fields(content, "opencode_model: zai/glm-5").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.model.as_deref(), Some("gpt-5"));
        assert_eq!(fm.opencode_model.as_deref(), Some("zai/glm-5"));
    }
}
