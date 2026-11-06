//! `agent-doc decision search` (`#decisionsearchmenu`): look an operator
//! answer up before asking again. Without FILE it searches the whole project
//! (`#decisionsearchnofile`).

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use agent_doc_preflight_io::answer_ledger::{
    DecisionSearchReport, ProjectDocuments, ProjectSearchScope, RegistrySearchStatus,
    search_decisions, search_project_decisions,
};
use agent_doc_preflight_io::answer_recall::RecallEnv;

/// What the command line asks to search.
#[derive(Debug, PartialEq, Eq)]
enum SearchTarget {
    Document { file: PathBuf, query: String },
    Project { query: String },
}

/// Split the positional words into an optional FILE and the query. FILE is
/// optional and the query needs no quotes, so only the first word can be FILE:
///
/// - `--project`: every word is the query.
/// - The first word names an existing session document: it is FILE, and a
///   query must follow it.
/// - Any other first word ending in `.md`: refused. It reads as a FILE that
///   does not exist or is not a session document, and searching the project
///   for it instead would report a false "not answered".
/// - Otherwise every word is the query, and the project is searched.
fn resolve_target(
    words: &[String],
    project: bool,
    names_session_document: &dyn Fn(&Path) -> bool,
) -> Result<SearchTarget> {
    let Some((first, rest)) = words.split_first() else {
        bail!("decision search needs a query");
    };
    if !project {
        if names_session_document(Path::new(first)) {
            if rest.is_empty() {
                bail!(
                    "{first} is a session document but no query follows it; pass a query after it, or use --project to search the project for these words"
                );
            }
            return Ok(SearchTarget::Document {
                file: PathBuf::from(first),
                query: rest.join(" "),
            });
        }
        if first.ends_with(".md") {
            bail!(
                "{first} is not a session document; fix the path, or use --project to search the project for these words"
            );
        }
    }
    Ok(SearchTarget::Project {
        query: words.join(" "),
    })
}

/// Whether `path` is an existing session document: opted in, or carrying
/// session structure (a done archive, or a document that predates the opt-in).
fn names_session_document(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|content| {
        agent_doc_frontmatter_io::session::is_agent_doc_document_for_file(&content, path)
            || crate::fleet_board_cmd::has_session_structure(&content)
    })
}

pub fn run_search(words: &[String], project: bool, limit: usize, json: bool) -> Result<()> {
    let env = RecallEnv::from_process();
    let report = match resolve_target(words, project, &names_session_document)? {
        SearchTarget::Document { file, query } => {
            search_decisions(&file, &file.display().to_string(), &query, &env, limit)?
        }
        SearchTarget::Project { query } => {
            let root = agent_doc_project_root_io::project_root_from_cwd()
                .context("decision search without FILE searches the current project")?;
            let scan = crate::skill::session_documents_under(&root);
            let documents = ProjectDocuments {
                documents: scan.documents,
                scan_exhausted: scan.exhausted,
            };
            search_project_decisions(&root, &documents, &query, &env, limit)?
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for line in render(&report) {
            println!("{line}");
        }
    }
    Ok(())
}

fn when(secs: Option<u64>) -> String {
    secs.filter(|secs| *secs > 0)
        .map(|secs| format!("{} UTC", agent_doc_log_time::format_human_timestamp(secs)))
        .unwrap_or_else(|| "first-seen time unknown".to_string())
}

/// The registry's share of a project search, as one phrase.
fn registry_phrase(registry: &RegistrySearchStatus) -> String {
    match registry {
        RegistrySearchStatus::Read { records } => format!("decisions.md ({records} records)"),
        RegistrySearchStatus::Missing => "no decisions.md".to_string(),
        RegistrySearchStatus::Unreadable { detail } => {
            format!("decisions.md, NOT searched (unreadable: {detail})")
        }
    }
}

/// What a project search covered, including what it did not.
fn scope_lines(scope: &ProjectSearchScope) -> Vec<String> {
    let mut lines = vec![format!(
        "searched {} session documents under {} and {}",
        scope.documents,
        scope.root,
        registry_phrase(&scope.registry)
    )];
    for skipped in &scope.skipped {
        lines.push(format!(
            "not searched: {} ({})",
            skipped.document, skipped.reason
        ));
    }
    if scope.scan_exhausted {
        lines.push(
            "the document scan hit its entry budget, so some documents may be missing; name one as FILE to search it"
                .to_string(),
        );
    }
    lines
}

pub fn render(report: &DecisionSearchReport) -> Vec<String> {
    let mut lines = Vec::new();
    if report.hits.is_empty() {
        lines.push(match &report.document {
            Some(document) => format!(
                "no answered decision in {document} matches “{}”: search the project with `agent-doc decision search <query>` (no file), then ask the operator, and say you searched",
                report.query
            ),
            None => format!(
                "no answered decision in this project matches “{}”: it has not been answered here; ask the operator, and say you searched",
                report.query
            ),
        });
    }
    for (rank, hit) in report.hits.iter().enumerate() {
        lines.push(format!(
            "{}. `{}` in {} [{}{}] {}",
            rank + 1,
            hit.decision_id,
            hit.document,
            hit.matched.as_str(),
            if hit.score < 1.0 {
                format!(" {:.2}", hit.score)
            } else {
                String::new()
            },
            hit.question
        ));
        lines.push(format!("   answer: “{}”", hit.answer));
        if !hit.is_current() {
            lines.push(if hit.superseded_by.is_empty() {
                "   no longer stands: do not act on this answer".to_string()
            } else {
                format!(
                    "   no longer stands: superseded by {}",
                    hit.superseded_by.join(", ")
                )
            });
        }
        if let Some(rationale) = &hit.rationale {
            lines.push(format!("   rationale: {rationale}"));
        }
        // A registry record states its own time in its location.
        lines.push(if hit.state.is_some() {
            format!("   given: {}", hit.location)
        } else {
            format!("   given: {} · {}", hit.location, when(hit.first_seen_at))
        });
        if !hit.earlier_answers.is_empty() {
            lines.push(format!(
                "   earlier: {}",
                hit.earlier_answers
                    .iter()
                    .map(|answer| format!("“{answer}”"))
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ));
        }
        if !hit.open_items.is_empty() {
            lines.push(format!(
                "   open copies: {}",
                hit.open_items
                    .iter()
                    .map(|item| format!("#{item}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Some(scope) = &report.project {
        lines.extend(scope_lines(scope));
    }
    lines.push(report.recall.summary());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(line: &str, project: bool) -> Result<SearchTarget> {
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        resolve_target(&words, project, &|path| path == Path::new("tasks/plan.md"))
    }

    fn document(query: &str) -> SearchTarget {
        SearchTarget::Document {
            file: PathBuf::from("tasks/plan.md"),
            query: query.to_string(),
        }
    }

    fn project(query: &str) -> SearchTarget {
        SearchTarget::Project {
            query: query.to_string(),
        }
    }

    /// The form that predates `#decisionsearchnofile` keeps its meaning.
    #[test]
    fn a_first_word_naming_a_session_document_is_file() {
        assert_eq!(
            resolve("tasks/plan.md port", false).unwrap(),
            document("port")
        );
        assert_eq!(
            resolve("tasks/plan.md which port now", false).unwrap(),
            document("which port now")
        );
    }

    #[test]
    fn with_no_file_every_word_is_the_query() {
        assert_eq!(resolve("port", false).unwrap(), project("port"));
        assert_eq!(
            resolve("which port now", false).unwrap(),
            project("which port now")
        );
        assert_eq!(resolve("#tdport", false).unwrap(), project("#tdport"));
        // A later word never reads as FILE.
        assert_eq!(
            resolve("port for tasks/plan.md", false).unwrap(),
            project("port for tasks/plan.md")
        );
    }

    /// A mistyped FILE must fail, not turn into a project search that reports
    /// the question as unanswered.
    #[test]
    fn a_first_word_that_reads_as_a_missing_file_is_refused() {
        for line in ["tasks/plann.md port", "tasks/plann.md"] {
            let err = resolve(line, false).unwrap_err().to_string();
            assert!(err.contains("not a session document"), "{err}");
            assert!(err.contains("--project"), "{err}");
        }
    }

    #[test]
    fn a_session_document_with_no_query_is_refused() {
        let err = resolve("tasks/plan.md", false).unwrap_err().to_string();
        assert!(err.contains("no query follows"), "{err}");
    }

    #[test]
    fn project_reads_every_word_as_the_query() {
        assert_eq!(
            resolve("tasks/plan.md port", true).unwrap(),
            project("tasks/plan.md port")
        );
        assert_eq!(
            resolve("tasks/plann.md", true).unwrap(),
            project("tasks/plann.md")
        );
        assert_eq!(
            resolve("tasks/plan.md", true).unwrap(),
            project("tasks/plan.md")
        );
    }
}
