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

    let findings = dialect_findings(file, content);

    classify_and_emit(file, &findings, mode, source, ops_logger)
}

/// Hide HTML-comment delimiters that agent-doc reads as prose from tagpath's
/// comment scanner (GH #93).
///
/// agent-doc decides whether marker-shaped text is structure with one shared
/// masking rule, [`agent_doc_element::element::find_marker_prose_ranges`]
/// (code fences, inline code, same-line backtick pairs, same-line quotes; GH
/// #90). tagpath's dialect lint pairs `<!--` with the next `-->` without that
/// rule, so a backticked marker *prefix* (`` `<!-- agent:` ``, no `-->`) opened
/// a comment that swallowed the component's own close and produced a false
/// `agent-doc/unclosed-component`. Every `<!--` whose start byte is prose is
/// rewritten to `<!__`, and its `-->` to `__>` when that closer lies inside the
/// same prose span. All replacements are single ASCII bytes, so tagpath's
/// line/column positions are unchanged. A comment opener outside prose is
/// never touched, so a real unclosed component still reaches the lint.
fn mask_prose_comment_delimiters(content: &str) -> std::borrow::Cow<'_, str> {
    const OPEN: &str = "<!--";
    const CLOSE: &str = "-->";
    if !content.contains(OPEN) {
        return std::borrow::Cow::Borrowed(content);
    }
    let prose = agent_doc_element::element::find_marker_prose_ranges(content);
    let mut bytes: Option<Vec<u8>> = None;
    let mut search_from = 0usize;
    while let Some(relative) = content[search_from..].find(OPEN) {
        let start = search_from + relative;
        search_from = start + OPEN.len();
        let Some(span_end) = prose
            .iter()
            .filter(|&&(span_start, span_end)| start >= span_start && start < span_end)
            .map(|&(_, span_end)| span_end)
            .max()
        else {
            continue;
        };
        let out = bytes.get_or_insert_with(|| content.as_bytes().to_vec());
        out[start + 2] = b'_';
        out[start + 3] = b'_';
        if let Some(close_relative) = content[search_from..span_end].find(CLOSE) {
            let close = search_from + close_relative;
            out[close] = b'_';
            out[close + 1] = b'_';
            search_from = close + CLOSE.len();
        }
    }
    match bytes {
        // Only ASCII `-` bytes were replaced with ASCII `_`, so UTF-8 holds.
        Some(bytes) => std::borrow::Cow::Owned(
            String::from_utf8(bytes).expect("ASCII-for-ASCII masking preserves UTF-8"),
        ),
        None => std::borrow::Cow::Borrowed(content),
    }
}

/// Pre-mutation dialect gate (GH #93).
///
/// `write --commit` used to run the dialect lint only on the final file state,
/// after the response had already been applied, so a blocking finding that
/// was already in the document left the response on disk, uncommitted, behind
/// an `INTERRUPTED` error. Callers run this against the pre-write document
/// before any capture or document mutation.
///
/// Only a finding the closeout cannot change refuses here: one in the body of
/// a component that `closeout_may_rewrite` rejects, or outside every component
/// and outside frontmatter. Such a finding is still in the final file state,
/// so the final gate would refuse it too; refusing now changes only *when*,
/// not *whether*. A finding inside a component the closeout rewrites (the
/// exchange and its boundary, a patched component, tracked work, the queue)
/// may be repaired by the write itself and is left to the final gate. Warnings
/// are not printed here (the final gate reports them once), and an `off` mode
/// is a silent no-op (the final gate logs the skip).
pub fn run_prewrite_dialect_gate_on_content_with_logger(
    file: &Path,
    content: &str,
    cli: Option<LintCliMode>,
    closeout_may_rewrite: &dyn Fn(&str) -> bool,
    ops_logger: OpsLogger,
) -> Result<()> {
    let (mode, source) = resolve_mode(file, content, cli);
    if mode == LintDialectMode::Off {
        return Ok(());
    }
    let errors: Vec<LintFinding> = dialect_findings(file, content)
        .into_iter()
        .filter(|finding| is_blocking(finding, mode))
        .collect();
    if errors.is_empty() {
        return Ok(());
    }
    let rewritable = closeout_rewritable_spans(content, closeout_may_rewrite);
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let errors: Vec<LintFinding> = errors
        .into_iter()
        .filter(|finding| {
            let Some(&offset) = finding.line.checked_sub(1).and_then(|l| line_starts.get(l)) else {
                // A position past the text cannot be located; leave it to
                // the final gate rather than refuse on a guess.
                return false;
            };
            !rewritable
                .iter()
                .any(|&(start, end)| offset >= start && offset < end)
        })
        .collect();
    if errors.is_empty() {
        return Ok(());
    }
    ops_logger(
        file,
        &format!(
            "lint_gate_blocked_prewrite file={} mode={} source={} errors={} mutated=false",
            file.display(),
            dialect_label(mode),
            source.as_str(),
            errors.len(),
        ),
    );
    Err(anyhow::anyhow!(
        "[lint-gate] INTERRUPTED before write: {} blocking lint finding(s) already in {} (mode={}, source={}). \
         Nothing was captured or written. Fix the directives below, then re-run the same \
         `agent-doc write --commit` / `agent-doc finalize`, or pass `--lint=off` for this write.\n{}",
        errors.len(),
        file.display(),
        dialect_label(mode),
        source.as_str(),
        format_findings_text(&errors)
    ))
}

/// Byte spans the closeout may rewrite: frontmatter, plus every component body
/// and closing marker whose name `closeout_may_rewrite` accepts. Opening
/// markers are excluded because ordinary closeout patching does not rewrite
/// their attributes; a pre-existing attribute finding must therefore refuse
/// before the response is applied. A document
/// whose component tree does not parse is treated as wholly rewritable, so the
/// pre-write gate never refuses on a tree the integrity gate has not vetted.
fn closeout_rewritable_spans(
    content: &str,
    closeout_may_rewrite: &dyn Fn(&str) -> bool,
) -> Vec<(usize, usize)> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return vec![(0, content.len())];
    };
    let mut spans: Vec<(usize, usize)> = components
        .iter()
        .filter(|component| closeout_may_rewrite(&component.name))
        .map(|component| (component.open_end, component.close_end))
        .collect();
    if let Some(rest) = content.strip_prefix("---\n") {
        let end = rest
            .find("\n---")
            .map_or(content.len(), |at| "---\n".len() + at + "\n---".len());
        spans.push((0, end));
    }
    spans
}

fn dialect_findings(file: &Path, content: &str) -> Vec<LintFinding> {
    let opts = AgentDocOptions {
        fs_checks: false,
        rule_filter: Vec::new(),
    };
    // GH #93: tagpath must see the same structure agent-doc's parser sees.
    let lint_content = mask_prose_comment_delimiters(content);
    reconcile_findings_with_agent_doc_registry(lint_agent_doc(file, &lint_content, &opts))
}

fn is_blocking(finding: &LintFinding, mode: LintDialectMode) -> bool {
    match finding.severity {
        LintSeverity::Error => true,
        LintSeverity::Warning => mode == LintDialectMode::Strict,
    }
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
    let components = agent_doc_element::element::parse(content)
        .with_context(|| {
            format!(
                "[integrity-gate] INTERRUPTED: malformed agent-doc component tree in {}; repair the document structure before retrying",
                file.display()
            )
        })?;
    validate_boundary_markers(file, content)?;
    if let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    {
        validate_exchange_response_bodies(file, exchange.content(content))?;
    }
    Ok(())
}

fn validate_exchange_response_bodies(file: &Path, exchange: &str) -> Result<()> {
    let mut active_heading: Option<&str> = None;
    let mut active_has_body = false;
    let mut fence: Option<&str> = None;

    for line in exchange.lines() {
        let trimmed = line.trim();
        let fence_marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        if let Some(marker) = fence_marker {
            if active_heading.is_some() {
                active_has_body = true;
            }
            fence = if fence == Some(marker) {
                None
            } else {
                Some(marker)
            };
            continue;
        }
        if fence.is_some() {
            if active_heading.is_some() && !trimmed.is_empty() {
                active_has_body = true;
            }
            continue;
        }

        let response_heading = trimmed
            .strip_prefix('❯')
            .map(str::trim_start)
            .unwrap_or(trimmed)
            .starts_with("### Re:");
        if response_heading {
            if let Some(previous) = active_heading
                && !active_has_body
            {
                bail!(
                    "[integrity-gate] INTERRUPTED: response heading `{}` in {} has no response body; prompt/response ordering was interrupted",
                    previous,
                    file.display(),
                );
            }
            active_heading = Some(trimmed);
            active_has_body = false;
            continue;
        }
        if trimmed.starts_with('❯') {
            if let Some(heading) = active_heading
                && !active_has_body
            {
                bail!(
                    "[integrity-gate] INTERRUPTED: prompt follows response heading `{}` without a response body in {}; prompt/response ordering was interrupted",
                    heading,
                    file.display(),
                );
            }
            continue;
        }
        if active_heading.is_none() || trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        active_has_body = true;
    }

    if let Some(heading) = active_heading
        && !active_has_body
    {
        bail!(
            "[integrity-gate] INTERRUPTED: response heading `{}` in {} has no response body; prompt/response ordering was interrupted",
            heading,
            file.display(),
        );
    }
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
        .filter(|finding| {
            !is_registry_known_unknown_component_finding(finding)
                && !is_agent_doc_known_attr_finding(finding)
                && !is_agent_doc_queue_bare_flag_finding(finding)
                && !is_agent_doc_done_bare_archive_finding(finding)
                && !is_explicit_no_preset_finding(finding)
        })
        .map(rewrite_empty_attr_value_hint)
        .collect()
}

/// Tagpath's attribute allowlist can lag agent-doc's domain. Reconcile its
/// `unknown-attr` findings against the queue crate's complete public queue and
/// status marker vocabularies. Unknown keys and known keys on the wrong
/// component remain findings.
fn is_agent_doc_known_attr_finding(finding: &LintFinding) -> bool {
    if finding.rule != "agent-doc/unknown-attr" {
        return false;
    }
    let mut quoted = finding.message.split('`');
    let Some(attribute) = quoted.nth(1) else {
        return false;
    };
    let Some(component) = quoted.nth(1) else {
        return false;
    };
    match component {
        "agent:queue" => agent_doc_queue::component_attrs::is_queue_component_attr(attribute),
        "agent:status" => agent_doc_queue::component_attrs::is_status_component_attr(attribute),
        _ => false,
    }
}

/// GH #227 (operator decision): `preset=""` on a prompt component
/// (`agent:queue`, `agent:exchange`) is an explicit "no preset", not a malformed
/// pair. The element attribute parser already drops an empty value, so every
/// preset resolver sees no preset; the lint gate must agree instead of blocking
/// compact/write on operator text agent-doc deliberately preserves.
///
/// Only `preset` gets this meaning. Every other empty value stays an error:
/// `patch=`, `archive=`, and backlog `queue=` need a value to mean anything, and
/// an empty `subagents=` / `fan-out=` is ambiguous (the bare flag means "all
/// eligible heads", while the parser drops the empty pair, i.e. "off").
fn is_explicit_no_preset_finding(finding: &LintFinding) -> bool {
    if finding.rule != "agent-doc/empty-attr-value" {
        return false;
    }
    let mut quoted = finding.message.split('`');
    let Some(attribute) = quoted.nth(1) else {
        return false;
    };
    let Some(component) = quoted.nth(1) else {
        return false;
    };
    attribute.trim_end_matches('=') == "preset"
        && matches!(component, "agent:queue" | "agent:exchange")
}

/// GH #227: tagpath's `agent-doc/empty-attr-value` hint says
/// ``provide a value: `key=<value>` `` — whose `<value>` placeholder is
/// swallowed as an HTML tag by editor notification renderers, and which hides
/// the usually-correct fix of deleting an attribute that carries nothing.
/// Replace it with an agent-doc hint that offers both and uses no angle-bracket
/// placeholder.
fn rewrite_empty_attr_value_hint(mut finding: LintFinding) -> LintFinding {
    if finding.rule != "agent-doc/empty-attr-value" {
        return finding;
    }
    let Some(key) = finding
        .message
        .split('`')
        .nth(1)
        .map(|token| token.trim_end_matches('='))
        .filter(|key| !key.is_empty())
    else {
        return finding;
    };
    finding.fix_hint = Some(empty_attr_value_hint(key));
    finding
}

fn empty_attr_value_hint(key: &str) -> String {
    format!("remove the empty `{key}=` attribute, or give it a value: `{key}=VALUE`")
}

/// Tagpath's generic attribute grammar requires values for attributes that the
/// queue scheduler accepts as bare flags. It therefore reports a misplaced
/// queue-only flag preserved on another component as malformed even though
/// preflight deliberately treats that token as ignored, warning-only input.
/// Reconcile the external finding against the queue domain's complete
/// vocabulary; unknown bare attributes and malformed value syntax still fail
/// closed.
fn is_agent_doc_queue_bare_flag_finding(finding: &LintFinding) -> bool {
    if finding.rule != "agent-doc/malformed-attr" {
        return false;
    }
    let mut quoted = finding.message.split('`');
    let Some(attribute) = quoted.nth(1) else {
        return false;
    };
    let Some(component) = quoted.nth(1) else {
        return false;
    };
    agent_doc_queue::component_attrs::is_recognized_bare_flag_attr(component, attribute)
}

/// A bare `archive` flag on `agent:done` selects the document-local default
/// `<document-basename>.done.md`. `archive=` remains an invalid empty pair.
fn is_agent_doc_done_bare_archive_finding(finding: &LintFinding) -> bool {
    if finding.rule != "agent-doc/malformed-attr" {
        return false;
    }
    let mut quoted = finding.message.split('`');
    let Some(attribute) = quoted.nth(1) else {
        return false;
    };
    let Some(component) = quoted.nth(1) else {
        return false;
    };
    attribute == "archive" && component == "agent:done"
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
    // Tagpath's dialect only knows the top-level component
    // set, so the nested markers the binary itself writes (a live cycle's
    // `agent:salient-response`, a `transfer --referral` block) would otherwise
    // block compact/write with "unknown agent-doc close tag".
    if agent_doc_element_registry::is_nested_binary_component(name) {
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
        if is_blocking(f, mode) {
            errors.push(f);
        } else {
            warnings.push(f);
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
         Error-severity findings block in every mode; strict mode also blocks warnings. \
         Fix the directives below, then retry the interrupted command, or \
         temporarily skip this gate with `--lint off` on `agent-doc write` / \
         `agent-doc compact`, `agent_doc_lint_dialect: off` in \
         frontmatter, or `[lint] dialect = \"off\"` in `.agent-doc/config.toml`.",
        errors_owned.len(),
        file.display(),
        dialect_label(mode),
        source.as_str()
    );
    let body = format_findings_text(&errors_owned);
    ops_logger(
        file,
        &format!(
            "lint_gate_blocked file={} mode={} source={} errors={} warnings={} first_rule={} first_line={} first_col={}",
            file.display(),
            dialect_label(mode),
            source.as_str(),
            errors_owned.len(),
            warnings.len(),
            errors_owned[0].rule,
            errors_owned[0].line,
            errors_owned[0].col,
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

    /// agent-doc-bugs.md 2026-09-29: a compacted summary quoting a stray ``` sat
    /// above a pasted fenced block; tagpath <= 0.12.2 paired the stray run with
    /// the fence opener, read the fence's closer as unclosed, and blocked every
    /// closeout with a false `agent-doc/unclosed-component`.
    #[test]
    fn stray_inline_backtick_run_above_a_pasted_fence_passes() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            - Prior summary/context: Fix api.md issue ``` \u{2022} Failed (exit 1)\n\
            Fix fpe.md issue\n\
            ```\n\
            \u{2022} pasted pane output\n\
            ```\n\
            \n\
            Your paste arrived empty (a bare `` `````` `` fence).\n\
            <!-- /agent:exchange -->\n";
        let file = write_doc(&dir, "stray-fence.md", doc);
        run(&file, None).expect("a mid-line backtick run must not unclose the exchange");
    }

    /// GH #93: a backlog item quoting a marker *prefix* (`<!-- agent:` with no
    /// `-->`) in backticks is prose to agent-doc's parser, but tagpath's comment
    /// scan read the backticked `<!--` as a comment opener, ran on to the
    /// component's own close, and reported `agent:backlog` as never closed.
    #[test]
    fn backticked_marker_prefix_in_backlog_item_is_prose() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_format: template\n---\n\n\
            ## Exchange\n\n\
            <!-- agent:exchange patch=append -->\n\
            \u{276f} hi\n\
            <!-- /agent:exchange -->\n\n\
            ## Backlog\n\n\
            <!-- agent:backlog -->\n\
            - [ ] [#b] only `<!-- agent:` occurrence inside backticks\n\
            <!-- /agent:backlog -->\n";
        let file = write_doc(&dir, "prefix.md", doc);
        run(&file, None).expect("a backticked marker prefix must not unclose the backlog");
    }

    /// Only openers agent-doc itself reads as prose are hidden from tagpath,
    /// byte offsets are preserved, and a complete quoted marker loses its
    /// closer too.
    #[test]
    fn prose_masking_hides_only_openers_agent_doc_reads_as_prose() {
        let doc = "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt `<!-- agent:` prose\n\
            <!-- /agent:exchange -->\n";
        let masked = mask_prose_comment_delimiters(doc);
        assert_eq!(
            masked.len(),
            doc.len(),
            "masking must preserve byte offsets"
        );
        assert!(
            !masked.contains("`<!--"),
            "backticked opener must be hidden: {masked}"
        );
        assert_eq!(
            masked.matches("<!--").count(),
            doc.matches("<!--").count() - 1,
            "only the prose opener may be hidden"
        );
        let complete = "a `<!-- agent:queue -->` b\n<!-- agent:x -->\n";
        let masked = mask_prose_comment_delimiters(complete);
        assert_eq!(masked, "a `<!__ agent:queue __>` b\n<!-- agent:x -->\n");
    }

    const UNKNOWN_COMPONENT_DOC: &str = "---\nagent_doc_session: test\n---\n\n\
        <!-- agent:exchange -->\n\
        prompt\n\
        <!-- /agent:exchange -->\n\n\
        <!-- agent:operator-notes -->\n\
        scratch\n\
        <!-- /agent:operator-notes -->\n";

    /// GH #93: the pre-write gate refuses a finding the closeout cannot
    /// change, names that nothing was written, and honours `off`.
    #[test]
    fn prewrite_dialect_gate_blocks_finding_outside_rewritten_components() {
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "bad.md", UNKNOWN_COMPONENT_DOC);
        let only_exchange = |name: &str| name == "exchange";
        let err = run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            UNKNOWN_COMPONENT_DOC,
            None,
            &only_exchange,
            noop_ops_logger,
        )
        .expect_err("a finding the write cannot repair must refuse before the write");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("INTERRUPTED before write")
                && msg.contains("Nothing was captured or written")
                && msg.contains("agent-doc/unknown-component"),
            "unexpected pre-write gate error: {msg}"
        );
        run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            UNKNOWN_COMPONENT_DOC,
            Some(LintCliMode::Off),
            &only_exchange,
            noop_ops_logger,
        )
        .expect("--lint=off must disable the pre-write dialect gate");
        run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            CLEAN_DOC,
            None,
            &only_exchange,
            noop_ops_logger,
        )
        .expect("a clean document must pass the pre-write gate");
    }

    /// A finding inside a component the closeout rewrites (here a malformed
    /// boundary the write replaces) is deferred to the final gate.
    #[test]
    fn prewrite_dialect_gate_defers_findings_the_write_may_repair() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- agent:boundary:head-boundary -->\n\
            <!-- /agent:exchange -->\n";
        let file = write_doc(&dir, "boundary.md", doc);
        let only_exchange = |name: &str| name == "exchange";
        run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            doc,
            None,
            &only_exchange,
            noop_ops_logger,
        )
        .expect("a finding inside the rewritten exchange must be left to the final gate");
        let nothing = |_: &str| false;
        run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            doc,
            None,
            &nothing,
            noop_ops_logger,
        )
        .expect_err("the same finding refuses when no component is rewritten");
    }

    /// GH #183: component-body patching does not rewrite the opening marker.
    /// A pre-existing malformed attribute must therefore refuse before the
    /// response is applied even when that component's body is a patch target.
    #[test]
    fn prewrite_dialect_gate_does_not_defer_open_marker_findings() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog auot -->\n\
            - [ ] [#b] tracked work\n\
            <!-- /agent:backlog -->\n";
        let file = write_doc(&dir, "open-marker-attr.md", doc);
        let only_backlog = |name: &str| name == "backlog";
        let error = run_prewrite_dialect_gate_on_content_with_logger(
            &file,
            doc,
            None,
            &only_backlog,
            noop_ops_logger,
        )
        .expect_err("a finding on an unchanged opening marker must refuse before write");
        let message = format!("{error:#}");
        assert!(message.contains("INTERRUPTED before write"), "{message}");
        assert!(message.contains("agent-doc/malformed-attr"), "{message}");
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
    fn ordered_prompt_response_exchange_passes_integrity_gate() {
        let dir = TempDir::new().unwrap();
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Rename the document.\n\n",
            "### Re: Rename the document.\n\n",
            "The live replica followed the new path.\n",
            "<!-- /agent:exchange -->\n",
        );
        let file = write_doc(&dir, "ordered-exchange.md", doc);
        validate_structure_on_content(&file, doc)
            .expect("prompt → response heading → response body must pass");
    }

    #[test]
    fn stranded_response_heading_blocks_integrity_gate() {
        let dir = TempDir::new().unwrap();
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ Rename the document.\n\n",
            "The response raced above its heading.\n\n",
            "### Re: Rename the document.\n",
            "<!-- /agent:exchange -->\n",
        );
        let file = write_doc(&dir, "stranded-heading.md", doc);
        let error = validate_structure_on_content(&file, doc)
            .expect_err("a response heading without its body must fail closed");
        assert!(
            format!("{error:#}").contains("has no response body"),
            "unexpected integrity error: {error:#}",
        );
    }

    #[test]
    fn prompt_after_empty_response_heading_blocks_integrity_gate() {
        let dir = TempDir::new().unwrap();
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ First prompt.\n\n",
            "### Re: First prompt.\n\n",
            "❯ Second prompt.\n",
            "<!-- /agent:exchange -->\n",
        );
        let file = write_doc(&dir, "prompt-after-empty-heading.md", doc);
        let error = validate_structure_on_content(&file, doc)
            .expect_err("a later prompt must not strand an empty response heading");
        assert!(
            format!("{error:#}").contains("prompt follows response heading"),
            "unexpected integrity error: {error:#}",
        );
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
    fn prompt_component_subagent_flags_are_valid_bare_attributes() {
        let dir = TempDir::new().unwrap();
        for attribute in ["subagents", "fan-out"] {
            let doc = format!(
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange -->\n\
                 prompt\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue {attribute} preset=\"#build\" priority go -->\n\
                 - do [#a]\n\
                 <!-- /agent:queue -->\n"
            );
            let file = write_doc(&dir, &format!("{attribute}.md"), &doc);
            run(&file, None).unwrap_or_else(|error| {
                panic!("bare queue flag `{attribute}` must pass lint: {error:#}")
            });

            let exchange_doc = format!(
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange {attribute} preset=\"#build\" -->\n\
                 prompt\n\
                 <!-- /agent:exchange -->\n"
            );
            let exchange_file = write_doc(&dir, &format!("exchange-{attribute}.md"), &exchange_doc);
            run(&exchange_file, None).unwrap_or_else(|error| {
                panic!("bare exchange flag `{attribute}` must pass lint: {error:#}")
            });
        }
    }

    /// GH #183: preflight warns that queue-only attributes on another
    /// component are ignored. The final tagpath adapter must not reinterpret
    /// the same preserved bare tokens as blocking malformed syntax.
    #[test]
    fn misplaced_bare_queue_attributes_match_preflight_warning_only_policy() {
        let dir = TempDir::new().unwrap();
        for attribute in [
            "auto",
            "preset",
            "start",
            "go",
            "stop",
            "subagents",
            "fan-out",
        ] {
            let doc = format!(
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange -->\n\
                 prompt\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:backlog {attribute} -->\n\
                 - [ ] [#b] tracked work\n\
                 <!-- /agent:backlog -->\n"
            );
            let file = write_doc(&dir, &format!("misplaced-{attribute}.md"), &doc);
            run(&file, None).unwrap_or_else(|error| {
                panic!("ignored bare queue attribute `{attribute}` must pass lint: {error:#}")
            });
        }
    }

    /// `#reviewpriority`: a bare `priority` on `agent:review` is the tracked-work
    /// ordering attribute, so lint must not block it as a malformed `key=value`
    /// pair even though tagpath's own allowlist predates it; a bare `priority`
    /// on a component with no priority ordering still fails closed.
    #[test]
    fn bare_priority_on_review_passes_lint_but_not_on_exchange() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:review priority -->\n\
            - [/] [#r] priority=2 gated work\n\
            <!-- /agent:review -->\n";
        let file = write_doc(&dir, "review-priority.md", doc);
        run(&file, None)
            .unwrap_or_else(|error| panic!("`agent:review priority` must pass lint: {error:#}"));
        run(&file, Some(LintCliMode::Strict)).unwrap_or_else(|error| {
            panic!("`agent:review priority` must pass strict lint: {error:#}")
        });

        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange priority -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n";
        let file = write_doc(&dir, "exchange-priority.md", doc);
        let error = run(&file, None).expect_err("bare priority on exchange must stay invalid");
        let message = format!("{error:#}");
        assert!(message.contains("agent-doc/malformed-attr"), "{message}");
        assert!(message.contains("attribute `priority`"), "{message}");
    }

    #[test]
    fn queue_attributes_that_require_values_still_fail_closed() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:queue preset priority go -->\n\
            - do [#a]\n\
            <!-- /agent:queue -->\n";
        let file = write_doc(&dir, "missing-preset-value.md", doc);
        let error = run(&file, None).expect_err("bare preset must remain invalid");
        let message = format!("{error:#}");
        assert!(message.contains("agent-doc/malformed-attr"), "{message}");
        assert!(message.contains("attribute `preset`"), "{message}");
        assert!(message.contains("missing `=value`"), "{message}");
    }

    #[test]
    fn valid_identified_queue_schedule_attributes_pass_lint() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:queue id=main -->\n\
            - immediate\n\
            <!-- /agent:queue -->\n\n\
            <!-- agent:queue id=issues interval=1h loop -->\n\
            - hourly\n\
            <!-- /agent:queue -->\n";
        let file = write_doc(&dir, "identified-schedules.md", doc);
        run(&file, None).unwrap_or_else(|error| {
            panic!("valid queue `id` / `interval` attributes must pass lint: {error:#}")
        });
        run(&file, Some(LintCliMode::Strict)).unwrap_or_else(|error| {
            panic!("valid queue `id` / `interval` attributes must pass strict lint: {error:#}")
        });
    }

    #[test]
    fn valid_status_sync_attributes_pass_lint() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:status sync=\"Keep release current\" sync-mode=additive interval=5m -->\n\
            operator note\n\
            <!-- /agent:status -->\n";
        let file = write_doc(&dir, "status-sync.md", doc);
        run(&file, None).unwrap_or_else(|error| {
            panic!("valid status sync attributes must pass lint: {error:#}")
        });
        run(&file, Some(LintCliMode::Strict)).unwrap_or_else(|error| {
            panic!("valid status sync attributes must pass strict lint: {error:#}")
        });
    }

    /// GH #227 (operator decision): `preset=""` means explicitly no preset. It
    /// must not block lint (and therefore compact/write) on queue or exchange.
    #[test]
    fn empty_preset_is_explicit_no_preset_and_passes_lint() {
        let dir = TempDir::new().unwrap();
        for (name, doc) in [
            (
                "queue.md",
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange -->\n\
                 prompt\n\
                 <!-- /agent:exchange -->\n\n\
                 <!-- agent:queue subagents preset=\"\" priority go -->\n\
                 - do [#a]\n\
                 <!-- /agent:queue -->\n",
            ),
            (
                "exchange.md",
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange preset=\"\" -->\n\
                 prompt\n\
                 <!-- /agent:exchange -->\n",
            ),
        ] {
            let file = write_doc(&dir, name, doc);
            run(&file, None).unwrap_or_else(|error| {
                panic!("`preset=\"\"` in {name} must pass lint: {error:#}")
            });
            run(&file, Some(LintCliMode::Strict)).unwrap_or_else(|error| {
                panic!("`preset=\"\"` in {name} must pass strict lint: {error:#}")
            });
        }
    }

    #[test]
    fn bare_done_archive_selects_document_local_default_and_passes_lint() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:done archive -->\n\
            <!-- /agent:done -->\n";
        let file = write_doc(&dir, "bare-archive.md", doc);
        run(&file, None).expect("bare archive flag must pass lint");
        run(&file, Some(LintCliMode::Strict)).expect("bare archive flag must pass strict lint");
    }

    /// Other empty attribute values stay malformed; their hint leads with
    /// removal, carries no `<value>` placeholder, and the header names
    /// `--lint off` on write/compact as an escape.
    #[test]
    fn other_empty_attr_values_still_block_with_actionable_hint() {
        let dir = TempDir::new().unwrap();
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:done archive=\"\" -->\n\
            <!-- /agent:done -->\n";
        let file = write_doc(&dir, "empty-archive.md", doc);
        let message = format!("{:#}", run(&file, None).expect_err("empty archive blocks"));
        assert!(message.contains("agent-doc/empty-attr-value"), "{message}");
        assert!(
            message.contains("hint: remove the empty `archive=` attribute"),
            "{message}"
        );
        assert!(!message.contains("<value>"), "{message}");
        assert!(message.contains("`--lint off`"), "{message}");
        assert!(message.contains("`agent-doc compact`"), "{message}");

        run(&file, Some(LintCliMode::Off)).expect("--lint off skips the dialect gate");
    }

    #[test]
    fn empty_non_preset_attr_hint_suggests_removal_or_value() {
        assert_eq!(
            empty_attr_value_hint("archive"),
            "remove the empty `archive=` attribute, or give it a value: `archive=VALUE`"
        );
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

    /// A live cycle's `salient-checkpoint` node sits inside
    /// `agent:exchange` until final response insertion removes it. Compact
    /// Exchange during that window refused with "unknown agent-doc close tag
    /// `/agent:salient-response`" and told the operator to fix a directive the
    /// binary had written; hand-deleting its markers then stranded the body as
    /// history. Render the node with the production renderer so the marker
    /// shape this test accepts is the shape the binary writes.
    #[test]
    fn live_salient_response_checkpoint_passes_integrity_and_dialect_lint() {
        let salient = agent_doc_markdown_ast::exchange_tree::render_salient_response(
            "cycle-1792051137843",
            "**Awaiting developer choice**\n\n1. **fix-first** — fix it first",
        );
        let doc = format!(
            "---\nagent_doc_session: test\n---\n\n\
             <!-- agent:exchange -->\n\
             ❯ consolidate the PRs\n\n\
             {salient}\
             <!-- agent:boundary:0123abcd -->\n\
             <!-- /agent:exchange -->\n"
        );
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "salient.md", &doc);
        let findings = dialect_findings(&file, &doc);
        assert!(
            findings.is_empty(),
            "a binary-written salient node must not produce dialect findings: {findings:?}"
        );
        run_on_content_with_logger(&file, &doc, Some(LintCliMode::Strict), noop_ops_logger)
            .expect("strict lint + integrity must accept a live salient-response node");
    }

    /// `transfer --referral` writes an `agent:referral` block into the target
    /// component; the same gate gap would refuse every later compact/write.
    #[test]
    fn transfer_referral_block_passes_dialect_lint() {
        let doc = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange -->\n\
            prompt\n\n\
            <!-- agent:referral src=\"../tsift.md\" component=\"exchange\" created=\"2026-10-15T00:00:00Z\" -->\n\
            *Context from [tsift.md](../tsift.md) — read source for full history.*\n\
            <!-- /agent:referral -->\n\
            <!-- /agent:exchange -->\n";
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "referral.md", doc);
        let findings = dialect_findings(&file, doc);
        assert!(findings.is_empty(), "{findings:?}");
        run_on_content_with_logger(&file, doc, Some(LintCliMode::Strict), noop_ops_logger)
            .expect("strict lint must accept a transfer referral block");
    }

    /// Markers that production code builds only as in-memory parse probes and
    /// never writes into a session document. Each needs a reason; anything the
    /// binary can persist must instead be lint-accepted.
    const IN_MEMORY_PROBE_MARKERS: &[(&str, &str)] = &[
        (
            "landability-probe",
            "agent-doc-template response_materialization wraps a body to parse it",
        ),
        (
            "scaffold",
            "agent-doc-template prompt_comment_repair wraps a prompt to parse it",
        ),
        (
            "patch",
            "agent-doc-template response_materialization detects a mistyped `agent:patch:` response marker; it is read, never written",
        ),
    ];

    fn workspace_production_sources() -> Vec<std::path::PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    out.push(path);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate lives in the agent-doc workspace")
            .to_path_buf();
        let mut src_dirs = vec![root.join("src")];
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("agent-doc-") {
                src_dirs.push(entry.path().join("src"));
            }
        }
        let mut files = Vec::new();
        for dir in src_dirs {
            walk(&dir, &mut files);
        }
        files.sort();
        files
    }

    /// Every `agent:NAME` marker literal in production (non-test, non-comment)
    /// source, with one file that writes it.
    fn binary_written_marker_names() -> std::collections::BTreeMap<String, std::path::PathBuf> {
        let mut names = std::collections::BTreeMap::new();
        for file in workspace_production_sources() {
            let source = std::fs::read_to_string(&file).unwrap_or_default();
            let production = source
                .split_once("#[cfg(test)]")
                .map_or(source.as_str(), |(before, _)| before);
            for line in production.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for prefix in ["<!-- agent:", "<!-- /agent:"] {
                    let mut rest = line;
                    while let Some(at) = rest.find(prefix) {
                        rest = &rest[at + prefix.len()..];
                        let name: String = rest
                            .chars()
                            .take_while(|ch| {
                                ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-'
                            })
                            .collect();
                        if !name.is_empty() {
                            names.entry(name).or_insert_with(|| file.clone());
                        }
                    }
                }
            }
        }
        names
    }

    /// Regression guard: the lint gate's known-marker set is
    /// checked against every marker the binary's own source writes, so adding
    /// a new binary-written marker without teaching the gate fails here instead
    /// of as an operator-facing "malformed directive" refusal.
    #[test]
    fn every_binary_written_marker_passes_the_dialect_lint() {
        let names = binary_written_marker_names();
        for required in [
            "exchange",
            "boundary",
            "queue",
            "salient-response",
            "referral",
        ] {
            assert!(
                names.contains_key(required),
                "source scan lost `agent:{required}`; the enumeration is broken: {:?}",
                names.keys().collect::<Vec<_>>()
            );
        }
        let dir = TempDir::new().unwrap();
        let file = write_doc(&dir, "markers.md", "");
        let mut rejected = Vec::new();
        for (name, source) in &names {
            if IN_MEMORY_PROBE_MARKERS
                .iter()
                .any(|(probe, _)| probe == name)
            {
                continue;
            }
            let doc = format!(
                "---\nagent_doc_session: test\n---\n\n\
                 <!-- agent:exchange -->\nprompt\n<!-- /agent:exchange -->\n\n\
                 <!-- agent:{name} -->\nbody\n<!-- /agent:{name} -->\n"
            );
            let unknown: Vec<_> = dialect_findings(&file, &doc)
                .into_iter()
                .filter(|finding| finding.rule == "agent-doc/unknown-component")
                .map(|finding| finding.message)
                .collect();
            if !unknown.is_empty() {
                rejected.push(format!(
                    "agent:{name} (written in {}): {unknown:?}",
                    source.display()
                ));
            }
        }
        assert!(
            rejected.is_empty(),
            "binary-written markers the lint gate rejects as unknown; register them in \
             agent-doc-element-registry (NESTED_BINARY_COMPONENTS for nested markers) or, \
             if they are never persisted, list them in IN_MEMORY_PROBE_MARKERS:\n{}",
            rejected.join("\n")
        );
    }
}
