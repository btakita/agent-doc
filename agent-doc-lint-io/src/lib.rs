//! # Module: lint_gate
//!
//! Session-document integrity and dialect gate. Every caller first validates
//! the agent-doc component tree, then invokes `tagpath lint --dialect
//! agent-doc` before any snapshot/commit boundary. Structural errors always
//! fail closed, including when the configurable dialect lint mode is `off`.
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

use anyhow::{Context, Result, bail};
use std::path::Path;

use agent_doc_frontmatter::{
    frontmatter::LintDialectMode,
    lint::{LintCliMode, LintModeSource, dialect_label, resolve_lint_mode},
};
use agent_doc_project_config_io as project_config_io;

use tagpath::lint::agent_doc::{
    AgentDocOptions, LintFinding, LintSeverity, format_findings_text, lint_agent_doc,
};

use agent_doc_element::ElementShape;

/// Best-effort lint marker logger used by production callers.
pub type OpsLogger = fn(&Path, &str);

fn noop_ops_logger(_: &Path, _: &str) {}

/// Resolve the effective lint mode given the optional CLI override, the
/// session document text (for frontmatter), and the workspace config.
///
/// Precedence: CLI > frontmatter > workspace config > default(`warn`).
pub fn resolve_mode(
    file: &Path,
    content: &str,
    cli: Option<LintCliMode>,
) -> (LintDialectMode, LintModeSource) {
    let project = project_config_io::load_project_for_doc(file);
    resolve_mode_from_project_dialect(content, cli, project.lint.dialect)
}

/// Resolve the effective lint mode using an already loaded project lint
/// dialect. Callers with cached project config can avoid redundant filesystem
/// reads through this adapter.
pub fn resolve_mode_from_project_dialect(
    content: &str,
    cli: Option<LintCliMode>,
    project_dialect: Option<LintDialectMode>,
) -> (LintDialectMode, LintModeSource) {
    resolve_lint_mode(content, cli, project_dialect)
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
    run_with_logger(file, cli, noop_ops_logger)
}

/// Run the finalize lint gate with an injected best-effort marker logger.
pub fn run_with_logger(file: &Path, cli: Option<LintCliMode>, ops_logger: OpsLogger) -> Result<()> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "lint_gate_document",
    )?;
    run_on_content_with_logger(file, &content, cli, ops_logger)
}

/// Run the finalize lint gate against explicit detached-disk authority.
pub fn run_force_disk_with_logger(
    file: &Path,
    cli: Option<LintCliMode>,
    ops_logger: OpsLogger,
) -> Result<()> {
    let content = agent_doc_document_realtime_io::resolve_disk_current_document_content(
        file,
        "lint_gate_document_force_disk",
    )?;
    run_on_content_with_logger(file, &content, cli, ops_logger)
}

/// Validate explicit authoritative content without re-reading the document.
///
/// This entrypoint lets preflight, compact, and session-check gate the exact
/// realtime projection they already resolved. Component-tree integrity is
/// mandatory; `off` disables only the configurable tagpath dialect policy.
pub fn run_on_content_with_logger(
    file: &Path,
    content: &str,
    cli: Option<LintCliMode>,
    ops_logger: OpsLogger,
) -> Result<()> {
    run_on_content(file, content, cli, ops_logger)
}

fn run_on_content(
    file: &Path,
    content: &str,
    cli: Option<LintCliMode>,
    ops_logger: OpsLogger,
) -> Result<()> {
    // Queue/backlog closeout briefly passes through a state where completed
    // backlog ids have been reaped but the matching queue directive has not
    // yet been removed. Structural corruption is never valid at that point;
    // cross-component orphan checks belong at authoritative transaction
    // boundaries (preflight/session-check), after the closeout converges.
    validate_structure_on_content(file, content)?;

    let (mode, source) = resolve_mode(file, content, cli);
    if mode == LintDialectMode::Off {
        ops_logger(
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
    let findings = reconcile_findings_with_agent_doc_registry(lint_agent_doc(file, content, &opts));

    classify_and_emit(file, &findings, mode, source, ops_logger)
}

/// Validate invariants that no policy mode may disable.
///
/// Preflight uses this boundary before its narrow legacy normalization passes;
/// final write/compact/session-check additionally run configurable tagpath lint.
pub fn validate_integrity_on_content_with_logger(
    file: &Path,
    content: &str,
    _ops_logger: OpsLogger,
) -> Result<()> {
    validate_structure_on_content(file, content)
}

/// Validate the component tree at every lint boundary, including transient
/// states inside one atomic closeout transaction.
pub fn validate_structure_on_content(file: &Path, content: &str) -> Result<()> {
    agent_doc_element::element::parse(content)
        .with_context(|| {
            format!(
                "[integrity-gate] INTERRUPTED: malformed agent-doc component tree in {}; repair the document structure before retrying",
                file.display()
            )
        })?;
    validate_boundary_markers(file, content)?;
    Ok(())
}

fn validate_boundary_markers(file: &Path, content: &str) -> Result<()> {
    const PREFIX: &str = "<!-- agent:boundary:";
    const SUFFIX: &str = " -->";

    let code_ranges = agent_doc_element::element::find_code_ranges(content);
    let mut search_from = 0;
    let mut count = 0;
    while let Some(relative_start) = content[search_from..].find(PREFIX) {
        let start = search_from + relative_start;
        if code_ranges
            .iter()
            .any(|&(code_start, code_end)| start >= code_start && start < code_end)
        {
            search_from = start + PREFIX.len();
            continue;
        }

        let suffix_search_start = start + PREFIX.len();
        let Some(relative_end) = content[suffix_search_start..].find(SUFFIX) else {
            bail!(
                "[integrity-gate] INTERRUPTED: malformed agent boundary marker in {}; historical partial patchback must be normalized before retrying",
                file.display()
            );
        };
        let end = suffix_search_start + relative_end + SUFFIX.len();
        let line_start = content[..start].rfind('\n').map_or(0, |pos| pos + 1);
        let line_end = content[end..]
            .find('\n')
            .map_or(content.len(), |relative| end + relative);
        let marker = &content[start..end];
        if content[line_start..line_end].trim() != marker {
            bail!(
                "[integrity-gate] INTERRUPTED: inline agent boundary marker in {}; the exchange must contain one standalone boundary marker",
                file.display()
            );
        }

        count += 1;
        search_from = end;
    }

    if count > 1 {
        bail!(
            "[integrity-gate] INTERRUPTED: {} agent boundary markers in {}; the exchange must contain at most one standalone boundary marker",
            count,
            file.display()
        );
    }
    Ok(())
}

fn reconcile_findings_with_agent_doc_registry(findings: Vec<LintFinding>) -> Vec<LintFinding> {
    findings
        .into_iter()
        .filter(|finding| !is_registry_known_unknown_component_finding(finding))
        .collect()
}

fn is_registry_known_unknown_component_finding(finding: &LintFinding) -> bool {
    if finding.rule != "agent-doc/unknown-component" {
        return false;
    }
    let Some(name) = unknown_component_name_from_message(&finding.message) else {
        return false;
    };
    registry_known_agent_marker_name(name)
}

fn unknown_component_name_from_message(message: &str) -> Option<&str> {
    let token = message.split('`').nth(1)?;
    let token = token.strip_prefix('/').unwrap_or(token);
    token.strip_prefix("agent:")
}

fn registry_known_agent_marker_name(name: &str) -> bool {
    if agent_doc_element_registry::find_built_in(name).is_some() {
        return true;
    }

    let Some((base, _suffix)) = name.split_once(':') else {
        return false;
    };
    agent_doc_element_registry::find_built_in(base)
        .is_some_and(|descriptor| descriptor.shape == ElementShape::InlineMarker)
}

fn classify_and_emit(
    file: &Path,
    findings: &[LintFinding],
    mode: LintDialectMode,
    source: LintModeSource,
    ops_logger: OpsLogger,
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
    ops_logger(
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
    fn clean_document_passes() {
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "clean.md", CLEAN_DOC);
        run(&file, None).expect("clean doc must pass lint gate");
    }

    #[test]
    fn notes_component_reconciles_against_agent_doc_registry() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:notes -->\n\
            operator-owned scratch state\n\
            <!-- /agent:notes -->\n";
        let file = write_doc(&dir, "notes.md", doc);
        run(&file, None).expect("registered notes component must pass lint gate");
    }

    #[test]
    fn malformed_component_tree_blocks_even_when_dialect_lint_is_off() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n\
<!-- agent:exchange -->\n\
prompt\n\
<!-- agent:notes -->\n\
operator-owned scratch state\n\
<!-- /agent:exchange -->\n\
<!-- /agent:notes -->\n";
        let file = write_doc(&dir, "invalid-tree.md", doc);
        let err = run(&file, None).expect_err("structural corruption must never be skippable");
        let message = format!("{err:#}");
        assert!(
            message.contains("[integrity-gate] INTERRUPTED")
                && message.contains("malformed agent-doc component tree"),
            "unexpected integrity error: {message}"
        );
    }

    #[test]
    fn duplicate_or_inline_boundaries_block_even_when_dialect_lint_is_off() {
        let dir = TempDir::new().unwrap();
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "prompt<!-- agent:boundary:inline -->\n",
            "<!-- agent:boundary:duplicate -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let file = write_doc(&dir, "duplicate-boundaries.md", doc);
        let err = run(&file, None).expect_err("boundary corruption must never be skippable");
        let message = format!("{err:#}");
        assert!(
            message.contains("[integrity-gate] INTERRUPTED")
                && message.contains("inline agent boundary marker"),
            "unexpected integrity error: {message}"
        );
    }

    #[test]
    fn boundary_literal_inside_fence_is_not_an_integrity_marker() {
        let dir = TempDir::new().unwrap();
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "```md\n<!-- agent:boundary:example -->\n```\n",
            "<!-- agent:boundary:real -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let file = write_doc(&dir, "boundary-literal.md", doc);
        run(&file, None).expect("code-fence literals plus one real boundary must pass");
    }

    #[test]
    fn boundary_marker_artifact_reconciles_against_inline_registry_element() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:boundary:a37c9696 -->\n\
            <!-- /agent:exchange -->\n";
        let file = write_doc(&dir, "boundary-artifact.md", doc);
        run(&file, None).expect("registered boundary inline marker artifact must pass lint gate");
    }

    #[test]
    fn unregistered_component_still_blocks_after_registry_reconciliation() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:operator-notes -->\n\
            scratch\n\
            <!-- /agent:operator-notes -->\n";
        let file = write_doc(&dir, "unknown.md", doc);
        let err = run(&file, None).expect_err("unregistered component must still block");
        let msg = format!("{err}");
        assert!(
            msg.contains("agent-doc/unknown-component"),
            "expected unknown-component rule in error, got: {msg}"
        );
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
