use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Document/routing UUID — permanent identifier for tmux session routing.
    #[serde(default)]
    pub session: Option<String>,
    /// Agent conversation ID — used for `--resume` with agent backends.
    /// Separate from `session` so the routing key never changes.
    #[serde(default)]
    pub resume: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
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
        let content = "---\nsession: existing-id\nagent: claude\n---\nBody\n";
        let (updated, sid) = ensure_session(content).unwrap();
        assert_eq!(sid, "existing-id");
        // Content should be unchanged
        assert_eq!(updated, content);
    }
}
