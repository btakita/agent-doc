use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Document format: controls document structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AgentDocFormat {
    /// Alternating ## User / ## Assistant blocks
    Append,
    /// In-place component patching with <!-- agent:name --> markers
    Template,
}

impl fmt::Display for AgentDocFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Append => write!(f, "append"),
            Self::Template => write!(f, "template"),
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
    /// Tmux session name for pane affinity (e.g., "claude").
    /// Set by `claim` or `sync` on first use; used to keep panes in the same session.
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

/// Update the tmux_session name in a document string.
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
        let content = "---\nsession: abc-123\nagent: claude\nmodel: opus\nbranch: main\n---\nBody\n";
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
        let result = set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }

    #[test]
    fn set_format_and_write_clears_deprecated() {
        let content = "---\nagent_doc_mode: append\n---\nBody\n";
        let result = set_format_and_write(content, AgentDocFormat::Template, AgentDocWrite::Crdt).unwrap();
        let (fm, _) = parse(&result).unwrap();
        assert!(fm.mode.is_none());
        assert_eq!(fm.format, Some(AgentDocFormat::Template));
        assert_eq!(fm.write_mode, Some(AgentDocWrite::Crdt));
    }
}
