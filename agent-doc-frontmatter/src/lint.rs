//! Pure lint dialect mode policy.
//!
//! This module owns CLI lint mode parsing, source labels, and precedence across
//! CLI, document frontmatter, project config, and defaults. File-backed config
//! loading and tagpath execution stay in orchestration.

use crate::frontmatter::{self, LintDialectMode};

/// CLI surface for the `--lint=...` flag on `agent-doc write` /
/// `agent-doc finalize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintCliMode {
    Off,
    Warn,
    Strict,
}

impl LintCliMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "warn" => Ok(Self::Warn),
            "strict" | "error" => Ok(Self::Strict),
            other => Err(format!(
                "invalid --lint value `{}` (expected off|warn|strict)",
                other
            )),
        }
    }

    pub const fn to_dialect(self) -> LintDialectMode {
        match self {
            Self::Off => LintDialectMode::Off,
            Self::Warn => LintDialectMode::Warn,
            Self::Strict => LintDialectMode::Strict,
        }
    }
}

/// Source that won the lint mode resolution chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintModeSource {
    Cli,
    Frontmatter,
    ProjectConfig,
    Default,
}

impl LintModeSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Frontmatter => "frontmatter",
            Self::ProjectConfig => "project_config",
            Self::Default => "default",
        }
    }
}

pub const fn dialect_label(mode: LintDialectMode) -> &'static str {
    match mode {
        LintDialectMode::Off => "off",
        LintDialectMode::Warn => "warn",
        LintDialectMode::Strict => "strict",
    }
}

/// Resolve the effective lint mode.
///
/// Precedence: CLI > frontmatter > project config > default(`warn`).
pub fn resolve_lint_mode(
    content: &str,
    cli: Option<LintCliMode>,
    project_mode: Option<LintDialectMode>,
) -> (LintDialectMode, LintModeSource) {
    if let Some(cli) = cli {
        return (cli.to_dialect(), LintModeSource::Cli);
    }
    if let Ok((fm, _)) = frontmatter::parse(content)
        && let Some(mode) = fm.agent_doc_lint_dialect
    {
        return (mode, LintModeSource::Frontmatter);
    }
    if let Some(mode) = project_mode {
        return (mode, LintModeSource::ProjectConfig);
    }
    (LintDialectMode::default(), LintModeSource::Default)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN_DOC: &str = "---\nagent_doc_session: test\n---\n\nbody\n";

    #[test]
    fn parse_cli_mode_accepts_error_alias() {
        assert_eq!(LintCliMode::parse("off").unwrap(), LintCliMode::Off);
        assert_eq!(LintCliMode::parse("warn").unwrap(), LintCliMode::Warn);
        assert_eq!(LintCliMode::parse("strict").unwrap(), LintCliMode::Strict);
        assert_eq!(LintCliMode::parse("error").unwrap(), LintCliMode::Strict);
        assert!(LintCliMode::parse("loud").is_err());
    }

    #[test]
    fn resolve_defaults_to_warn() {
        assert_eq!(
            resolve_lint_mode(CLEAN_DOC, None, None),
            (LintDialectMode::Warn, LintModeSource::Default)
        );
    }

    #[test]
    fn resolve_uses_project_when_frontmatter_absent() {
        assert_eq!(
            resolve_lint_mode(CLEAN_DOC, None, Some(LintDialectMode::Off)),
            (LintDialectMode::Off, LintModeSource::ProjectConfig)
        );
    }

    #[test]
    fn resolve_frontmatter_overrides_project() {
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: strict\n---\n\nbody\n";
        assert_eq!(
            resolve_lint_mode(doc, None, Some(LintDialectMode::Off)),
            (LintDialectMode::Strict, LintModeSource::Frontmatter)
        );
    }

    #[test]
    fn resolve_cli_overrides_frontmatter_and_project() {
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: strict\n---\n\nbody\n";
        assert_eq!(
            resolve_lint_mode(doc, Some(LintCliMode::Off), Some(LintDialectMode::Warn)),
            (LintDialectMode::Off, LintModeSource::Cli)
        );
    }

    #[test]
    fn dialect_label_matches_serialized_modes() {
        assert_eq!(dialect_label(LintDialectMode::Off), "off");
        assert_eq!(dialect_label(LintDialectMode::Warn), "warn");
        assert_eq!(dialect_label(LintDialectMode::Strict), "strict");
    }
}
