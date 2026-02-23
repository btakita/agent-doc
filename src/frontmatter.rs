use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(default)]
    pub session: Option<String>,
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
