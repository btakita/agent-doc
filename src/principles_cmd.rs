//! # Module: principles_cmd
//!
//! `agent-doc principles <FILE> [--json]` (`#firstprincipleslist`): the
//! read-only listing of the operator first principles in scope for one
//! session document, with the warnings their authoring produces. It also
//! lists the `fact [#id]:` premises in scope (`#firstprinciplesfactlist`).
//!
//! ## Spec
//!
//! - The listing composes principles with the same function preflight uses
//!   for the cycle contract,
//!   [`agent_doc_preflight_command_io::composed_rules_and_principles`]:
//!   the global principles file, every ancestor `.agent-doc/project.md`
//!   (farthest first), then the document. What it lists is exactly what the
//!   next cycle contract carries as `principles`, and its warnings are exactly
//!   the `principle_*` and `fact_*` preflight warnings.
//! - Facts come from the same composition (`Composition::facts`, built by
//!   `agent_doc_principles::rederive::compose_facts`, the reader premise
//!   change detection uses): every ancestor `.agent-doc/project.md` and the
//!   document, nearest wins with the replaced sources in `overrides`. A
//!   second fact with one id in one file is a `fact_duplicate_id` warning; a
//!   fact in the global principles file is a `fact_in_global_file` warning.
//! - The document text is the current authoritative projection (the same
//!   read `agent-doc lint` and `agent-doc outline` use), so unsaved editor
//!   text counts.
//! - Text output lists each principle as `[#id] <scope>: <statement>` with its
//!   rationale, source, and the sources it overrides, then each fact the same
//!   way, then the warnings. `--json` prints
//!   `{file, principles, facts, warnings}`; `principles` uses the
//!   cycle-contract shape, each fact is `{id, statement, scope, source,
//!   overrides?}`, and each warning is `{code, message, source}`.
//! - Warnings never change the exit status: principle problems never fail
//!   admission, so they never fail the listing either.
//! - `agent-doc lint` runs the same checks ([`emit_lint_warnings`]) and prints
//!   each principle and fact warning on stderr after the lint gate. They stay
//!   warnings in every lint mode.

use std::path::Path;

use agent_doc_principles::rederive::Fact;
use agent_doc_principles::{Composition, Principle, PrincipleIssue};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct PrinciplesListing<'a> {
    file: String,
    principles: &'a [Principle],
    facts: &'a [Fact],
    warnings: &'a [PrincipleIssue],
}

pub fn run(file: &Path, json: bool) -> Result<()> {
    let composition = compose(file, "principles_command_document")?;
    if json {
        println!("{}", render_json(file, &composition)?);
    } else {
        print!("{}", render_text(file, &composition));
    }
    Ok(())
}

/// Print the principle warnings for `file` on stderr, for `agent-doc lint`.
///
/// Best effort: a document that cannot be read was already reported by the
/// lint gate, so nothing is printed for it here.
pub fn emit_lint_warnings(file: &Path) {
    let Ok(composition) = compose(file, "lint_principles_document") else {
        return;
    };
    eprint!("{}", render_lint_warnings(file, &composition.issues));
}

fn compose(file: &Path, source: &str) -> Result<Composition> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let content =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)?;
    Ok(agent_doc_preflight_command_io::composed_rules_and_principles(file, &content))
}

fn render_json(file: &Path, composition: &Composition) -> Result<String> {
    Ok(serde_json::to_string_pretty(&PrinciplesListing {
        file: file.display().to_string(),
        principles: &composition.principles,
        facts: &composition.facts,
        warnings: &composition.issues,
    })?)
}

fn render_text(file: &Path, composition: &Composition) -> String {
    let mut out = String::new();
    if composition.principles.is_empty() {
        out.push_str(&format!(
            "No principles in scope for {}.\n\
             Add `principle [#id]: <statement>` items to an agent:rules block in the document, \
             an ancestor .agent-doc/project.md, or {}.\n",
            file.display(),
            agent_doc_config::global_principles_path().display()
        ));
    } else {
        out.push_str(&format!(
            "{} principle(s) in scope for {} (nearest scope wins):\n",
            composition.principles.len(),
            file.display()
        ));
        for principle in &composition.principles {
            out.push_str(&format!(
                "\n[#{}] {}: {}\n",
                principle.id,
                principle.scope.as_str(),
                indent_continuation(&principle.statement, "  ")
            ));
            if let Some(rationale) = &principle.rationale {
                out.push_str(&format!(
                    "  rationale: {}\n",
                    indent_continuation(rationale, "    ")
                ));
            }
            out.push_str(&format!("  source: {}\n", principle.source));
            for overridden in &principle.overrides {
                out.push_str(&format!("  overrides: {overridden}\n"));
            }
        }
    }
    if composition.facts.is_empty() {
        out.push_str(
            "\nNo facts in scope. Add `fact [#id]: <statement>` items to an agent:rules block \
             in the document or an ancestor .agent-doc/project.md.\n",
        );
    } else {
        out.push_str(&format!(
            "\n{} fact(s) in scope for {} (nearest scope wins):\n",
            composition.facts.len(),
            file.display()
        ));
        for fact in &composition.facts {
            out.push_str(&format!(
                "\n[#{}] {}: {}\n",
                fact.id,
                fact.scope.as_str(),
                indent_continuation(&fact.statement, "  ")
            ));
            out.push_str(&format!("  source: {}\n", fact.source));
            for overridden in &fact.overrides {
                out.push_str(&format!("  overrides: {overridden}\n"));
            }
        }
    }
    if !composition.issues.is_empty() {
        out.push_str(&format!("\n{} warning(s):\n", composition.issues.len()));
        for issue in &composition.issues {
            out.push_str(&format!(
                "  {}: {} ({})\n",
                issue.code, issue.message, issue.source
            ));
        }
    }
    out
}

fn render_lint_warnings(file: &Path, issues: &[PrincipleIssue]) -> String {
    if issues.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "[lint] {} principle/fact warning(s) for {} (never blocking; list them with `agent-doc principles {}`):\n",
        issues.len(),
        file.display(),
        file.display()
    );
    for issue in issues {
        out.push_str(&format!(
            "{}: warning: {} [{}]\n",
            issue.source, issue.message, issue.code
        ));
    }
    out
}

fn indent_continuation(text: &str, indent: &str) -> String {
    text.lines()
        .collect::<Vec<_>>()
        .join(&format!("\n{indent}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_principles::{Layer, PrincipleScope};

    fn sample() -> Composition {
        agent_doc_principles::compose(&[
            Layer {
                scope: PrincipleScope::Global,
                source: "/config/agent-doc/principles.md".into(),
                rules: vec![
                    "principle [#velocity]: Velocity over polish.".into(),
                    "principle [#no-reask]: Never re-ask an answered question.\n  rationale: It spends attention twice.\n  and costs trust.".into(),
                ],
            },
            Layer {
                scope: PrincipleScope::Project,
                source: "/repo/.agent-doc/project.md".into(),
                rules: vec![
                    "principle [#velocity]: Correctness over velocity.\n  scope: global".into(),
                    "fact [#jws]: Credentials are JWS.".into(),
                ],
            },
            Layer {
                scope: PrincipleScope::Document,
                source: "/repo/plan.md".into(),
                rules: vec![
                    "principle: forgot the id".into(),
                    "fact [#jws]: Credentials are JWS\n  with a kid header.".into(),
                    "fact [#jws]: declared twice".into(),
                ],
            },
        ])
    }

    #[test]
    fn text_lists_each_principle_with_scope_source_overrides_and_warnings() {
        let text = render_text(Path::new("/repo/plan.md"), &sample());
        assert_eq!(
            text,
            concat!(
                "2 principle(s) in scope for /repo/plan.md (nearest scope wins):\n",
                "\n[#velocity] project: Correctness over velocity.\n",
                "  source: /repo/.agent-doc/project.md\n",
                "  overrides: /config/agent-doc/principles.md\n",
                "\n[#no-reask] global: Never re-ask an answered question.\n",
                "  rationale: It spends attention twice.\n    and costs trust.\n",
                "  source: /config/agent-doc/principles.md\n",
                "\n1 fact(s) in scope for /repo/plan.md (nearest scope wins):\n",
                "\n[#jws] document: Credentials are JWS\n  with a kid header.\n",
                "  source: /repo/plan.md\n",
                "  overrides: /repo/.agent-doc/project.md\n",
                "\n3 warning(s):\n",
                "  principle_scope_mismatch: principle [#velocity] declares scope global but is written in a project file, so it applies as project; move it to the global principles file (~/.config/agent-doc/principles.md) to apply it in every project (/repo/.agent-doc/project.md)\n",
                "  principle_malformed: a principle needs an id: `principle [#id]: <statement>`; kept as an ordinary rule: \"principle: forgot the id\" (/repo/plan.md)\n",
                "  fact_duplicate_id: fact [#jws] is declared twice in this file; the first one applies (/repo/plan.md)\n",
            )
        );
    }

    #[test]
    fn text_without_principles_says_where_to_write_one() {
        let text = render_text(Path::new("plan.md"), &Composition::default());
        assert!(
            text.starts_with("No principles in scope for plan.md.\n"),
            "{text}"
        );
        assert!(text.contains("principles.md"), "{text}");
        assert!(
            text.contains("\nNo facts in scope. Add `fact [#id]: <statement>`"),
            "{text}"
        );
        assert!(!text.contains("warning"), "{text}");
    }

    #[test]
    fn json_uses_the_cycle_contract_principle_shape() {
        let json: serde_json::Value =
            serde_json::from_str(&render_json(Path::new("/repo/plan.md"), &sample()).unwrap())
                .unwrap();
        assert_eq!(json["file"], "/repo/plan.md");
        assert_eq!(json["principles"][0]["id"], "velocity");
        assert_eq!(json["principles"][0]["scope"], "project");
        assert_eq!(
            json["principles"][0]["overrides"],
            serde_json::json!(["/config/agent-doc/principles.md"])
        );
        assert!(json["principles"][1].get("overrides").is_none(), "{json:#}");
        assert_eq!(json["warnings"][0]["code"], "principle_scope_mismatch");
        assert_eq!(json["warnings"][1]["code"], "principle_malformed");
        assert_eq!(json["warnings"][1]["source"], "/repo/plan.md");
        assert_eq!(
            json["facts"],
            serde_json::json!([{
                "id": "jws",
                "statement": "Credentials are JWS\nwith a kid header.",
                "scope": "document",
                "source": "/repo/plan.md",
                "overrides": ["/repo/.agent-doc/project.md"],
            }])
        );
        assert_eq!(json["warnings"][2]["code"], "fact_duplicate_id");
    }

    #[test]
    fn lint_warnings_name_source_code_and_listing_command() {
        let text = render_lint_warnings(Path::new("plan.md"), &sample().issues);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(lines[0].contains("3 principle/fact warning(s) for plan.md"));
        assert!(lines[0].contains("agent-doc principles plan.md"));
        assert!(lines[1].starts_with("/repo/.agent-doc/project.md: warning: "));
        assert!(lines[1].ends_with("[principle_scope_mismatch]"));
        assert!(lines[2].ends_with("[principle_malformed]"));
        assert!(lines[3].starts_with("/repo/plan.md: warning: fact [#jws] is declared twice"));
        assert!(lines[3].ends_with("[fact_duplicate_id]"));
        assert_eq!(render_lint_warnings(Path::new("plan.md"), &[]), "");
    }
}
