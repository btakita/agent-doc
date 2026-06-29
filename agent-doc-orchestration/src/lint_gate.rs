//! # Module: lint_gate
//!
//! Finalize lint gate that invokes `tagpath lint --dialect agent-doc` on
//! the session document before the snapshot/commit boundary. Errors fail
//! the cycle closed; warnings surface on stderr but do not block (unless
//! the dialect mode is `strict`).
//!
//! ## Spec
//!
//! - `run(file, override_mode)` reads the session document, resolves the
//!   effective lint mode (CLI override > frontmatter > workspace config >
//!   default `warn`), invokes the in-process tagpath lint library when the
//!   mode is not `off`, and returns:
//!   - `Ok(())` on clean lint or warnings-only in `warn` mode.
//!   - `Err(LintGateError::Findings { ... })` on error-class findings (and
//!     on warning-class findings when the mode is `strict`).
//!
//! - The integration is a **library call**, not a subprocess. This keeps the
//!   gate type-safe and avoids subprocess overhead on every finalize.
//!
//! - The error message format mirrors `tagpath`'s CLI output:
//!   `<path>:<line>:<col> <severity>: <message> [<rule>]` with an optional
//!   `  hint: <fix_hint>` follow-up line. Each finding is preserved so the
//!   user can fix multiple issues in one round-trip.
//!
//! ## Agentic Contracts
//!
//! - `LintCliMode` is the CLI surface (`--lint=off|warn|strict`). It is
//!   distinct from `LintDialectMode` so we can express "no CLI override"
//!   explicitly via `Option<LintCliMode>`.
//!
//! - The gate is **never** skipped silently: if `resolve_mode` returns `Off`
//!   from any source, the resolution is logged via `ops_log` so a missed
//!   blocker can be traced back to the source.
//!
//! - The lint library entrypoint (`tagpath::lint::agent_doc::lint_agent_doc`)
//!   does not perform filesystem-dependent checks by default; the gate keeps
//!   `fs_checks = false` so finalize cannot fail closed on transient
//!   archive-target state.

use anyhow::Result;
use std::path::Path;

use crate::project_config_io as project_config;
use agent_doc_frontmatter::frontmatter::{self, LintDialectMode};

use tagpath::lint::agent_doc::{
    AgentDocOptions, LintFinding, LintSeverity, format_findings_text, lint_agent_doc,
};

/// CLI surface for the `--lint=...` flag on `agent-doc write` /
/// `agent-doc finalize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintCliMode {
    Off,
    Warn,
    Strict,
}

impl LintCliMode {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "off" => Ok(LintCliMode::Off),
            "warn" => Ok(LintCliMode::Warn),
            "strict" | "error" => Ok(LintCliMode::Strict),
            other => Err(format!(
                "invalid --lint value `{}` (expected off|warn|strict)",
                other
            )),
        }
    }

    fn to_dialect(self) -> LintDialectMode {
        match self {
            LintCliMode::Off => LintDialectMode::Off,
            LintCliMode::Warn => LintDialectMode::Warn,
            LintCliMode::Strict => LintDialectMode::Strict,
        }
    }
}

/// Source that won the resolution chain. Used for logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintModeSource {
    Cli,
    Frontmatter,
    ProjectConfig,
    Default,
}

impl LintModeSource {
    fn as_str(self) -> &'static str {
        match self {
            LintModeSource::Cli => "cli",
            LintModeSource::Frontmatter => "frontmatter",
            LintModeSource::ProjectConfig => "project_config",
            LintModeSource::Default => "default",
        }
    }
}

/// Resolve the effective lint mode given the optional CLI override, the
/// session document text (for frontmatter), and the workspace config.
///
/// Precedence: CLI > frontmatter > workspace config > default(`warn`).
pub fn resolve_mode(
    file: &Path,
    content: &str,
    cli: Option<LintCliMode>,
) -> (LintDialectMode, LintModeSource) {
    if let Some(cli) = cli {
        return (cli.to_dialect(), LintModeSource::Cli);
    }
    if let Ok((fm, _)) = frontmatter::parse(content)
        && let Some(mode) = fm.agent_doc_lint_dialect
    {
        return (mode, LintModeSource::Frontmatter);
    }
    let project = project_config::load_project_for_doc(file);
    if let Some(mode) = project.lint.dialect {
        return (mode, LintModeSource::ProjectConfig);
    }
    (LintDialectMode::default(), LintModeSource::Default)
}

/// Resolve the effective lint mode using a cached [`RunContext`] for the
/// workspace config lookup. Avoids redundant filesystem reads when the
/// caller already has a context from a prior phase.
pub fn resolve_mode_with_context(
    rc: &crate::graph::RunContext,
    content: &str,
    cli: Option<LintCliMode>,
) -> (LintDialectMode, LintModeSource) {
    if let Some(cli) = cli {
        return (cli.to_dialect(), LintModeSource::Cli);
    }
    if let Ok((fm, _)) = frontmatter::parse(content)
        && let Some(mode) = fm.agent_doc_lint_dialect
    {
        return (mode, LintModeSource::Frontmatter);
    }
    let config = rc.project_config();
    if let Some(mode) = config.lint.dialect {
        return (mode, LintModeSource::ProjectConfig);
    }
    (LintDialectMode::default(), LintModeSource::Default)
}

/// Run the finalize lint gate for `file`, with an optional CLI override.
///
/// Returns `Ok(())` on:
///   - mode = `Off` (gate skipped, with `ops_log` audit).
///   - no findings.
///   - warnings-only in `Warn` mode (warnings printed to stderr).
///
/// Returns `Err` when blocking findings are present.
pub fn run(file: &Path, cli: Option<LintCliMode>) -> Result<()> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            // The session file is expected to exist by the time the lint
            // gate runs; missing files are upstream bugs. Surface the error
            // so finalize can fail closed instead of silently skipping the
            // gate.
            return Err(anyhow::anyhow!(
                "[lint-gate] failed to read session document {}: {}",
                file.display(),
                e
            ));
        }
    };

    let (mode, source) = resolve_mode(file, &content, cli);
    if mode == LintDialectMode::Off {
        crate::ops_log::log_op(
            file,
            &format!(
                "lint_gate_skipped file={} source={}",
                file.display(),
                source.as_str()
            ),
        );
        return Ok(());
    }

    let opts = AgentDocOptions {
        fs_checks: false,
        rule_filter: Vec::new(),
    };
    let findings = lint_agent_doc(file, &content, &opts);

    classify_and_emit(file, &findings, mode, source)
}

fn classify_and_emit(
    file: &Path,
    findings: &[LintFinding],
    mode: LintDialectMode,
    source: LintModeSource,
) -> Result<()> {
    if findings.is_empty() {
        return Ok(());
    }

    let mut errors: Vec<&LintFinding> = Vec::new();
    let mut warnings: Vec<&LintFinding> = Vec::new();
    for f in findings {
        match f.severity {
            LintSeverity::Error => errors.push(f),
            LintSeverity::Warning => match mode {
                LintDialectMode::Strict => errors.push(f),
                _ => warnings.push(f),
            },
        }
    }

    if !warnings.is_empty() {
        let warnings_owned: Vec<LintFinding> = warnings.iter().map(|f| (*f).clone()).collect();
        eprintln!(
            "[lint-gate] {} warning(s) for {} (source={}):",
            warnings_owned.len(),
            file.display(),
            source.as_str()
        );
        eprint!("{}", format_findings_text(&warnings_owned));
    }

    if errors.is_empty() {
        return Ok(());
    }

    let errors_owned: Vec<LintFinding> = errors.iter().map(|f| (*f).clone()).collect();
    let header = format!(
        "[lint-gate] INTERRUPTED: {} blocking lint finding(s) for {} (mode={}, source={}). \
         Fix the directives below before re-running `agent-doc finalize` / \
         `agent-doc write --commit`, or set `agent_doc_lint_dialect: off` in \
         frontmatter / `[lint] dialect = \"off\"` in `.agent-doc/config.toml` \
         to temporarily skip this gate.",
        errors_owned.len(),
        file.display(),
        dialect_label(mode),
        source.as_str()
    );
    let body = format_findings_text(&errors_owned);
    crate::ops_log::log_op(
        file,
        &format!(
            "lint_gate_blocked file={} mode={} source={} errors={} warnings={}",
            file.display(),
            dialect_label(mode),
            source.as_str(),
            errors_owned.len(),
            warnings.len()
        ),
    );
    Err(anyhow::anyhow!("{}\n{}", header, body))
}

fn dialect_label(mode: LintDialectMode) -> &'static str {
    match mode {
        LintDialectMode::Off => "off",
        LintDialectMode::Warn => "warn",
        LintDialectMode::Strict => "strict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_doc(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let p = dir.path().join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        p
    }

    const CLEAN_DOC: &str = "---\nagent_doc_session: test\n---\n\n\
        <!-- agent:exchange -->\n\
        hello\n\
        <!-- /agent:exchange -->\n";

    const MALFORMED_DOC: &str = "---\nagent_doc_session: test\n---\n\n\
        <!-- agent:exchange -->\n\
        prompt\n\
        <!-- /agent:exchange -->\n\
        <!-- agent:done archive tasks/x.done.md -->\n\
        <!-- /agent:done -->\n";

    #[test]
    fn parse_cli_mode() {
        assert_eq!(LintCliMode::parse("off").unwrap(), LintCliMode::Off);
        assert_eq!(LintCliMode::parse("warn").unwrap(), LintCliMode::Warn);
        assert_eq!(LintCliMode::parse("strict").unwrap(), LintCliMode::Strict);
        assert_eq!(LintCliMode::parse("error").unwrap(), LintCliMode::Strict);
        assert!(LintCliMode::parse("loud").is_err());
    }

    #[test]
    fn clean_document_passes() {
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "clean.md", CLEAN_DOC);
        run(&file, None).expect("clean doc must pass lint gate");
    }

    #[test]
    fn malformed_directive_blocks() {
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "bad.md", MALFORMED_DOC);
        let err = run(&file, None).expect_err("malformed directive must block");
        let msg = format!("{}", err);
        assert!(
            msg.contains("agent-doc/malformed-attr"),
            "expected malformed-attr rule in error, got: {msg}"
        );
        assert!(msg.contains("INTERRUPTED"), "expected INTERRUPTED prefix");
    }

    #[test]
    fn frontmatter_off_skips_gate() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:done archive tasks/x.done.md -->\n\
            <!-- /agent:done -->\n";
        let file = write_doc(&dir, "off.md", doc);
        run(&file, None).expect("frontmatter off must skip gate");
    }

    #[test]
    fn cli_off_overrides_frontmatter_strict() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: strict\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:done archive tasks/x.done.md -->\n\
            <!-- /agent:done -->\n";
        let file = write_doc(&dir, "cli_off.md", doc);
        // CLI off must win over frontmatter strict.
        run(&file, Some(LintCliMode::Off)).expect("CLI off must override frontmatter strict");
    }

    #[test]
    fn cli_strict_overrides_frontmatter_warn() {
        // Document with a warning-class finding
        // (`agent-doc/unknown-patch-marker` is a warning per tagpath
        // agent-doc dialect). Under `warn` mode the gate passes; under
        // `strict` (via CLI) it must fail closed.
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- patch:nonsense -->\n\
            body\n\
            <!-- /patch:nonsense -->\n\
            <!-- /agent:exchange -->\n";
        let file = write_doc(&dir, "warn.md", doc);
        // Warn mode: gate passes (warnings on stderr).
        run(&file, Some(LintCliMode::Warn)).expect("warning-only doc must pass under warn mode");
        // Strict mode: gate fails.
        let err = run(&file, Some(LintCliMode::Strict))
            .expect_err("warning must escalate to error under strict mode");
        let msg = format!("{}", err);
        assert!(
            msg.contains("unknown-patch-marker") || msg.contains("INTERRUPTED"),
            "expected blocked finding under strict, got: {msg}"
        );
    }

    #[test]
    fn resolve_default_when_no_overrides() {
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "default.md", CLEAN_DOC);
        let content = std::fs::read_to_string(&file).unwrap();
        let (mode, source) = resolve_mode(&file, &content, None);
        assert_eq!(mode, LintDialectMode::Warn);
        assert_eq!(source, LintModeSource::Default);
    }

    #[test]
    fn resolve_frontmatter_when_set() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: strict\n---\n\nbody\n";
        let file = write_doc(&dir, "fm.md", doc);
        let content = std::fs::read_to_string(&file).unwrap();
        let (mode, source) = resolve_mode(&file, &content, None);
        assert_eq!(mode, LintDialectMode::Strict);
        assert_eq!(source, LintModeSource::Frontmatter);
    }

    #[test]
    fn resolve_cli_when_set() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: strict\n---\n\nbody\n";
        let file = write_doc(&dir, "cli.md", doc);
        let content = std::fs::read_to_string(&file).unwrap();
        let (mode, source) = resolve_mode(&file, &content, Some(LintCliMode::Off));
        assert_eq!(mode, LintDialectMode::Off);
        assert_eq!(source, LintModeSource::Cli);
    }

    #[test]
    fn project_config_dialect_off_skips_gate() {
        // Create a project root with .agent-doc/config.toml setting lint
        // dialect = "off", and place a session doc inside it.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(&agent_doc_dir).unwrap();
        std::fs::write(
            agent_doc_dir.join("config.toml"),
            "[lint]\ndialect = \"off\"\n",
        )
        .unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:done archive tasks/x.done.md -->\n\
            <!-- /agent:done -->\n";
        let file = write_doc(&dir, "proj.md", doc);
        run(&file, None).expect("project [lint] dialect = off must skip gate");
    }
}
