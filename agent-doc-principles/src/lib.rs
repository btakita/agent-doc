//! Operator first principles (`#firstprinciples`).
//!
//! Principles are the top layer of the decision hierarchy: principles, then
//! standing decisions, then decisions, then tracked items. A principle is an
//! operator-authored value or priority with an id, a statement, an optional
//! rationale, and a scope. The binary pushes every principle in scope into the
//! cycle contract, the same way it pushes `agent_rules`, so the agent never has
//! to look for them.
//!
//! This crate is pure: callers read the files and hand in their rule texts.
//! [`derivation`] holds the derivation graph model (premises, inference step,
//! conclusion, review status), the "why was this decided" walk, change impact,
//! and contradiction checks.
//!
//! ## Authoring
//!
//! A principle is an `agent:rules` list item (or free-text rule) whose first
//! line has the form `principle [#id]: <statement>` (`principle` in any case;
//! `#id` without brackets is also accepted). Indented continuation lines may
//! carry fields:
//!
//! ```markdown
//! <!-- agent:rules -->
//! - principle [#editor-authority]: Editor-visible operator text is authoritative.
//!   rationale: The operator reads and edits the editor buffer; disk can lag.
//!   scope: project
//! - Run the focused test before the full suite.
//! <!-- /agent:rules -->
//! ```
//!
//! - Continuation lines before the first field extend the statement. A
//!   `rationale:` line starts the rationale; later non-field lines extend it.
//!   A `scope:` line declares the scope (`global`, `project`, `document`; the
//!   alias `session` means `document`). A field line may carry a leading `- `.
//! - A principle item is not also an `agent_rules` entry.
//! - A rule that starts like a principle but is malformed (no id, an invalid
//!   id, or an empty statement) stays an ordinary rule and yields a
//!   [`PrincipleIssue`], so nothing the operator wrote is dropped.
//!
//! ## Scope
//!
//! The effective scope is where the principle is written, not what it
//! declares: the global principles file is `global`, every
//! `.agent-doc/project.md` is `project`, the session document is `document`.
//! A declared `scope:` that disagrees yields a `principle_scope_mismatch`
//! issue naming where to move it; the source scope still wins, because the
//! binary never copies a principle into a file the operator did not write it
//! in.
//!
//! ## Composition
//!
//! [`compose`] takes layers farthest first (global, ancestor projects farthest
//! first, the nearest project, the session document). Plain rules of project
//! and document layers concatenate in that order, exactly as before
//! principles existed. A global layer holds principles only; a plain rule in
//! it is reported and ignored. A nearer principle with the same id replaces
//! the farther one in place (the list keeps first-declared order) and records
//! the replaced source in [`Principle::overrides`]. Two principles with one id
//! in the same layer: the first wins and the second is reported.
//!
//! [`compose`] also composes the `fact [#id]: <statement>` items of the same
//! layers ([`rederive::compose_facts`], `#firstprinciplesfactlist`): project
//! and document layers only, nearest wins with `overrides`, a duplicate id in
//! one file reported as `fact_duplicate_id`, and a fact in the global file
//! reported as `fact_in_global_file` and ignored. Fact items stay in `rules`.

pub mod derivation;
pub mod rederive;

use serde::{Deserialize, Serialize};

/// Where a principle applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipleScope {
    /// Every project of this operator (the global principles file).
    Global,
    /// Every document under one project root (`.agent-doc/project.md`).
    Project,
    /// One session document (its own `agent:rules`).
    Document,
}

impl PrincipleScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
            Self::Document => "document",
        }
    }

    /// Parse a declared `scope:` value.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "global" => Some(Self::Global),
            "project" => Some(Self::Project),
            "document" | "session" => Some(Self::Document),
            _ => None,
        }
    }
}

/// One principle in scope for this cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principle {
    pub id: String,
    pub statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub scope: PrincipleScope,
    /// The file the principle was read from.
    pub source: String,
    /// Sources of farther principles with this id that this one replaced,
    /// farthest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<String>,
}

/// A principle as written, before composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPrinciple {
    pub id: String,
    pub statement: String,
    pub rationale: Option<String>,
    /// The `scope:` field as written, if any.
    pub declared_scope: Option<String>,
}

/// How one rule text reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleForm {
    /// An ordinary rule.
    Plain,
    /// A well-formed principle.
    Principle(ParsedPrinciple),
    /// Starts like a principle but is not one; the rule stays plain.
    Malformed { code: &'static str, reason: String },
}

pub const ISSUE_MALFORMED: &str = "principle_malformed";
pub const ISSUE_UNKNOWN_SCOPE: &str = "principle_unknown_scope";
pub const ISSUE_SCOPE_MISMATCH: &str = "principle_scope_mismatch";
pub const ISSUE_DUPLICATE_ID: &str = "principle_duplicate_id";
pub const ISSUE_GLOBAL_PLAIN_RULE: &str = "principle_global_plain_rule";
/// A second fact with one id in the same file (`#firstprinciplesfactlist`).
pub const ISSUE_FACT_DUPLICATE_ID: &str = "fact_duplicate_id";
/// A fact written in the global principles file, which holds principles only
/// (`#firstprinciplesfactlist`).
pub const ISSUE_FACT_IN_GLOBAL_FILE: &str = "fact_in_global_file";

/// A problem with what the operator wrote. Surfaced as a preflight warning;
/// never fails the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrincipleIssue {
    pub code: &'static str,
    pub message: String,
    pub source: String,
}

/// One source of rules, in composition order.
#[derive(Debug, Clone)]
pub struct Layer {
    pub scope: PrincipleScope,
    pub source: String,
    /// Rule texts from every `agent:rules` body in the source, in order.
    pub rules: Vec<String>,
}

/// Composed rules, principles, and facts for one cycle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composition {
    pub rules: Vec<String>,
    pub principles: Vec<Principle>,
    /// Facts in scope ([`rederive::compose_facts`]); fact items also stay in
    /// `rules` as written.
    pub facts: Vec<rederive::Fact>,
    /// Principle issues in layer order, then fact issues.
    pub issues: Vec<PrincipleIssue>,
}

/// How the agent applies a non-empty `principles` list.
pub const PRINCIPLES_GUIDANCE: &str = "\
principles are the operator's first principles, the top of the decision \
hierarchy (principles, then standing decisions, then decisions, then items). \
Check them before asking the operator anything. When the principles settle a \
question, decide without asking: cite each one as [#id] and state the \
inference in one sentence (`Derived from [#id]: <why it settles this>`) so \
the operator can review or overturn it. When they bear on a question but do \
not settle it, ask once: list the bearing principles by [#id], say what \
premise is missing, and give the answer they recommend. Never act against a \
principle silently; if a request conflicts with one, name it and the \
conflict. A nearer scope replaces a farther principle with the same id \
(document over project over global). Content-only edits of principles do not \
start a turn.";

/// Guidance for `principles`, present only when the list is non-empty.
pub fn principles_guidance(principles: &[Principle]) -> Option<String> {
    (!principles.is_empty()).then(|| PRINCIPLES_GUIDANCE.to_string())
}

/// Whether `id` is a valid principle id: ASCII letters, digits, `-`, `_`, `.`,
/// starting with a letter or digit.
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Classify one rule text.
pub fn parse_rule(rule: &str) -> RuleForm {
    let mut lines = rule.lines();
    let Some(first) = lines.next() else {
        return RuleForm::Plain;
    };
    let Some(after_keyword) = strip_keyword(first.trim_start()) else {
        return RuleForm::Plain;
    };
    let rest = after_keyword.trim_start();
    let (id, statement_head) = if let Some(bracketed) = rest.strip_prefix("[#") {
        let Some((id, tail)) = bracketed.split_once(']') else {
            return malformed("the `[#id]` has no closing `]`");
        };
        let Some(statement) = tail.trim_start().strip_prefix(':') else {
            return malformed("expected `:` after `[#id]`");
        };
        (id.trim(), statement)
    } else if let Some(hashed) = rest.strip_prefix('#') {
        let Some((id, statement)) = hashed.split_once(':') else {
            return malformed("expected `:` after `#id`");
        };
        (id.trim(), statement)
    } else if rest.starts_with(':') {
        return malformed("a principle needs an id: `principle [#id]: <statement>`");
    } else {
        return RuleForm::Plain;
    };
    if id.is_empty() {
        return malformed("a principle needs an id: `principle [#id]: <statement>`");
    }
    if !is_valid_id(id) {
        return malformed(&format!(
            "principle id {id:?} may use only ASCII letters, digits, `-`, `_` and `.`"
        ));
    }

    let mut statement = vec![statement_head.trim().to_string()];
    let mut rationale: Option<Vec<String>> = None;
    let mut declared_scope = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let field_text = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .unwrap_or(trimmed);
        if let Some(value) = strip_field(field_text, "rationale") {
            rationale = Some(vec![value.to_string()]);
        } else if let Some(value) = strip_field(field_text, "scope") {
            declared_scope = Some(value.to_string());
        } else if let Some(rationale) = rationale.as_mut() {
            rationale.push(trimmed.to_string());
        } else {
            statement.push(trimmed.to_string());
        }
    }
    let statement = join_nonempty(&statement);
    if statement.is_empty() {
        return malformed(&format!("principle [#{id}] has no statement"));
    }
    RuleForm::Principle(ParsedPrinciple {
        id: id.to_string(),
        statement,
        rationale: rationale
            .map(|lines| join_nonempty(&lines))
            .filter(|text| !text.is_empty()),
        declared_scope,
    })
}

fn malformed(reason: &str) -> RuleForm {
    RuleForm::Malformed {
        code: ISSUE_MALFORMED,
        reason: reason.to_string(),
    }
}

/// `principle` (any case) followed by whitespace, `[`, `#`, or `:`.
fn strip_keyword(line: &str) -> Option<&str> {
    const KEYWORD: &str = "principle";
    let head = line.get(..KEYWORD.len())?;
    if !head.eq_ignore_ascii_case(KEYWORD) {
        return None;
    }
    let rest = &line[KEYWORD.len()..];
    let next = rest.chars().next()?;
    (next.is_whitespace() || matches!(next, '[' | '#' | ':')).then_some(rest)
}

fn strip_field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let head = text.get(..name.len())?;
    if !head.eq_ignore_ascii_case(name) {
        return None;
    }
    text[name.len()..]
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
}

fn join_nonempty(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compose layers given farthest first. See the crate docs.
pub fn compose(layers: &[Layer]) -> Composition {
    let mut composition = Composition::default();
    for layer in layers {
        let mut seen_in_layer: Vec<String> = Vec::new();
        for rule in &layer.rules {
            match parse_rule(rule) {
                RuleForm::Plain => {
                    if layer.scope == PrincipleScope::Global
                        && let Some(fact) = rederive::parse_fact(rule)
                    {
                        composition.issues.push(PrincipleIssue {
                            code: ISSUE_FACT_IN_GLOBAL_FILE,
                            message: format!(
                                "the global principles file holds principles only; ignored fact [#{}]. Move it to a project's .agent-doc/project.md agent:rules, or the session document's.",
                                fact.id
                            ),
                            source: layer.source.clone(),
                        });
                    } else if layer.scope == PrincipleScope::Global {
                        composition.issues.push(PrincipleIssue {
                            code: ISSUE_GLOBAL_PLAIN_RULE,
                            message: format!(
                                "the global principles file holds principles only; ignored the rule {:?}. Write it as `principle [#id]: <statement>`, or move it to a project's .agent-doc/project.md agent:rules.",
                                first_line(rule)
                            ),
                            source: layer.source.clone(),
                        });
                    } else {
                        composition.rules.push(rule.clone());
                    }
                }
                RuleForm::Malformed { code, reason } => {
                    let global = layer.scope == PrincipleScope::Global;
                    composition.issues.push(PrincipleIssue {
                        code,
                        message: format!(
                            "{reason}; {} {:?}",
                            if global {
                                "ignored"
                            } else {
                                "kept as an ordinary rule:"
                            },
                            first_line(rule)
                        ),
                        source: layer.source.clone(),
                    });
                    if !global {
                        composition.rules.push(rule.clone());
                    }
                }
                RuleForm::Principle(parsed) => {
                    if seen_in_layer.contains(&parsed.id) {
                        composition.issues.push(PrincipleIssue {
                            code: ISSUE_DUPLICATE_ID,
                            message: format!(
                                "principle [#{}] is declared twice in this file; the first one applies",
                                parsed.id
                            ),
                            source: layer.source.clone(),
                        });
                        continue;
                    }
                    seen_in_layer.push(parsed.id.clone());
                    check_declared_scope(&parsed, layer, &mut composition.issues);
                    let principle = Principle {
                        id: parsed.id,
                        statement: parsed.statement,
                        rationale: parsed.rationale,
                        scope: layer.scope,
                        source: layer.source.clone(),
                        overrides: Vec::new(),
                    };
                    match composition
                        .principles
                        .iter_mut()
                        .find(|existing| existing.id == principle.id)
                    {
                        Some(existing) => {
                            let mut overrides = std::mem::take(&mut existing.overrides);
                            overrides.push(existing.source.clone());
                            *existing = Principle {
                                overrides,
                                ..principle
                            };
                        }
                        None => composition.principles.push(principle),
                    }
                }
            }
        }
    }
    let facts = rederive::compose_facts(layers);
    composition.facts = facts.facts;
    composition.issues.extend(facts.issues);
    composition
}

fn check_declared_scope(parsed: &ParsedPrinciple, layer: &Layer, issues: &mut Vec<PrincipleIssue>) {
    let Some(declared) = parsed.declared_scope.as_deref() else {
        return;
    };
    let Some(declared_scope) = PrincipleScope::parse(declared) else {
        issues.push(PrincipleIssue {
            code: ISSUE_UNKNOWN_SCOPE,
            message: format!(
                "principle [#{}] declares scope {declared:?}; expected global, project, or document. It applies as {}.",
                parsed.id,
                layer.scope.as_str()
            ),
            source: layer.source.clone(),
        });
        return;
    };
    if declared_scope == layer.scope {
        return;
    }
    let advice = match declared_scope {
        PrincipleScope::Global => {
            "move it to the global principles file (~/.config/agent-doc/principles.md) to apply it in every project"
        }
        PrincipleScope::Project => {
            "move it to the project's .agent-doc/project.md agent:rules to apply it to every document in the project"
        }
        PrincipleScope::Document => {
            "move it to the session document's agent:rules to apply it to that document only"
        }
    };
    issues.push(PrincipleIssue {
        code: ISSUE_SCOPE_MISMATCH,
        message: format!(
            "principle [#{}] declares scope {} but is written in a {} file, so it applies as {}; {advice}",
            parsed.id,
            declared_scope.as_str(),
            layer.scope.as_str(),
            layer.scope.as_str()
        ),
        source: layer.source.clone(),
    });
}

fn first_line(rule: &str) -> &str {
    rule.lines().next().unwrap_or_default().trim()
}

#[cfg(test)]
mod tests;
