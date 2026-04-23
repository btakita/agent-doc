//! # Module: frontmatter
//!
//! ## Spec
//! - Defines the YAML frontmatter schema (`Frontmatter`) embedded between `---\n` delimiters at
//!   the top of agent-doc documents.
//! - `parse(content)` splits a document string into `(Frontmatter, body_str)`. Documents that do
//!   not start with `---\n` return a default `Frontmatter` and the full content as body.
//!   Unterminated frontmatter (no closing `---`) returns `Err`.
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
//! - Field renames and aliases ensure backward compatibility:
//!   `session` / `agent_doc_session` → `session`;
//!   `mode` / `response_mode` / `agent_doc_mode` → `mode`.
//!
//! ## Agentic Contracts
//! - `parse` never panics; it returns `Err` only for an unterminated frontmatter block or a YAML
//!   deserialisation failure. All missing optional fields default to `None`.
//! - `write(parse(doc).0, parse(doc).1)` round-trips the document: re-parsing the result yields
//!   semantically equivalent `Frontmatter` and an identical body string.
//! - `ensure_session` is idempotent: calling it twice on the same document returns the same
//!   session ID and does not modify the document on the second call.
//! - `merge_fields` is additive: it never removes existing fields unless an explicit `null`
//!   value is supplied for that field in `yaml_fields`.
//! - `set_format_and_write` is the only function that actively clears a field (`mode = None`);
//!   all other helpers are strictly additive.
//! - `resolve_mode` is pure and total: it always returns a valid `ResolvedMode` regardless of
//!   which combination of deprecated and current fields are populated.
//! - Serialised YAML always uses the canonical field names (`agent_doc_session`,
//!   `agent_doc_mode`, `agent_doc_format`, `agent_doc_write`); legacy aliases are read-only.
//!
//! ## Evals
//! - parse_no_frontmatter: content without `---` prefix → default Frontmatter, full content as body
//! - parse_all_fields: document with all known fields → each field correctly populated
//! - parse_null_fields: YAML `null` values → all fields `None`
//! - parse_unterminated_frontmatter: `---` open with no close → `Err` "Unterminated frontmatter"
//! - parse_closing_at_eof: closing `---` without trailing newline → body = ""
//! - write_roundtrip: write then parse → identical Frontmatter, body contains original text
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

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::model_tier::Tier;

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

#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// Auto-compact threshold: line count for the exchange component.
    /// When the exchange component exceeds this many lines, preflight automatically
    /// runs compact before computing the diff. Set to 0 or omit to disable.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "agent_doc_auto_compact"
    )]
    pub auto_compact: Option<usize>,
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
}

impl Frontmatter {
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
}

/// Parse YAML frontmatter from a document. Returns (frontmatter, body).
/// If no frontmatter block is present, returns defaults and the full content as body.
pub fn parse(content: &str) -> Result<(Frontmatter, &str)> {
    if !content.starts_with("---\n") {
        return Ok((Frontmatter::default(), content));
    }
    let rest = &content[4..]; // skip opening ---\n
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---"))
        .ok_or_else(|| anyhow::anyhow!("Unterminated frontmatter block"))?;
    let yaml = &rest[..end];
    let fm: Frontmatter = serde_yaml::from_str(yaml)?;
    let body_start = 4 + end + 4; // opening --- + yaml + closing ---\n
    let body = if body_start <= content.len() {
        &content[body_start..]
    } else {
        ""
    };
    Ok((fm, body))
}

/// Write frontmatter back into a document, preserving the body.
pub fn write(fm: &Frontmatter, body: &str) -> Result<String> {
    let yaml = serde_yaml::to_string(fm)?;
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
            "agent_args" => fm.agent_args = val_str(),
            "claude_args" => fm.claude_args = val_str(),
            "codex_args" => fm.codex_args = val_str(),
            _ => {
                eprintln!("[frontmatter] ignoring unknown patch field: {}", key_str);
            }
        }
    }

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

/// Read the session UUID from a document file. Returns empty string if not found.
pub fn read_session_id(file: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    let (fm, _) = parse(&content).ok()?;
    fm.session
}

#[cfg(test)]
mod tests {
    use super::*;

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
            branch: Some("dev".to_string()),
            tmux_session: None,
            mode: None,
            format: None,
            write_mode: None,
            stream_config: None,
            agent_args: None,
            claude_args: None,
            codex_args: None,
            no_mcp: None,
            enable_tool_search: None,
            debounce_ms: None,
            links: vec![],
            auto_compact: None,
            model_tier: None,
            hooks: std::collections::HashMap::new(),
            env: indexmap::IndexMap::new(),
            agent_doc_env_inherit: None,
            cwd: None,
        };
        let body = "# Hello\n\nBody text.\n";
        let written = write(&fm, body).unwrap();
        let (fm2, body2) = parse(&written).unwrap();
        assert_eq!(fm2.session, fm.session);
        assert_eq!(fm2.agent, fm.agent);
        assert_eq!(fm2.model, fm.model);
        assert_eq!(fm2.branch, fm.branch);
        // Roundtrip preserves body (may have leading newline from parse)
        assert!(body2.contains("# Hello"));
        assert!(body2.contains("Body text."));
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
    }

    #[test]
    fn parse_agent_args_and_harness_aliases() {
        let content = "---\nagent_args: \"--json\"\nclaude_args: \"--dangerously-skip-permissions\"\ncodex_args: \"-s danger-full-access\"\n---\nBody\n";
        let (fm, _) = parse(content).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("--json"));
        assert_eq!(
            fm.claude_args.as_deref(),
            Some("--dangerously-skip-permissions")
        );
        assert_eq!(fm.codex_args.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn write_roundtrip_agent_args() {
        let fm = Frontmatter {
            agent_args: Some("--json -s workspace-write".to_string()),
            codex_args: Some("-s danger-full-access".to_string()),
            ..Default::default()
        };
        let written = write(&fm, "body\n").unwrap();
        let (fm2, _) = parse(&written).unwrap();
        assert_eq!(fm2.agent_args.as_deref(), Some("--json -s workspace-write"));
        assert_eq!(fm2.codex_args.as_deref(), Some("-s danger-full-access"));
    }

    #[test]
    fn merge_fields_agent_args() {
        let content = "---\nclaude_args: old\ncodex_args: old-codex\n---\nBody\n";
        let result = merge_fields(content, "agent_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("new"));
        assert_eq!(fm.claude_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("old-codex"));
    }

    #[test]
    fn merge_fields_codex_args() {
        let content = "---\nagent_args: old\n---\nBody\n";
        let result = merge_fields(content, "codex_args: new").unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert_eq!(fm.agent_args.as_deref(), Some("old"));
        assert_eq!(fm.codex_args.as_deref(), Some("new"));
    }
}
