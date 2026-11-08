//! The no-drift contract every managed list shares (`#managedlistnodrift`).
//!
//! A managed list — `agent:active`, `agent:worktrees`, the lifecycle markers on
//! `agent:run` / `agent:queue` rows, `agent:todo`, `agent:backlog`,
//! `agent:review`, `agent:icebox`, and `agent:done` — is a projection of
//! durable state:
//!
//! ```text
//! render(durable_state) -> component text
//! ```
//!
//! Rendered text is output. A projection must never read its own earlier
//! output back as input, or a stale render becomes self-sustaining evidence
//! for itself (the 2026-11-03 `agent:active` ghosts: a visible `☑️` row was
//! treated as "terminal document authority", so no durable change could ever
//! retire it, and `repair-managed-state` compared the projection against
//! itself and reported "already synchronized").
//!
//! This module holds what every list shares:
//!
//! - [`ManagedList`]: the list kinds and the components they own.
//! - [`component_bodies`] / [`replace_component_body`]: full component replace.
//! - [`ListMismatch`] / [`check_list`]: the `session-check` invariant
//!   `rendered == render(durable_state)`, reported as the exact first differing
//!   line of the component.
//! - [`ReconcileAction`] / [`missing_operator_lines`]: what a reconcile pass
//!   may do, and the check behind the hard invariant that operator-typed text
//!   is adopted, never deleted or rewritten.
//! - [`laws`]: the two laws each list's property tests assert — a second render
//!   is byte-identical, and rendering after each lifecycle event equals a
//!   from-scratch recompute.
//!
//! Everything here is pure text-in/text-out.

use anyhow::{Context, Result};

/// One managed list kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ManagedList {
    /// `agent:active`: running and needs-integration work, from claims,
    /// release receipts, and lineage.
    Active,
    /// `agent:worktrees`: live worktrees for this document (topic, branch,
    /// directory), from lineage receipts whose directory still exists and
    /// whose attempt has no `removed` receipt. Operator list children are
    /// actions; this render keeps them until preflight consumes them.
    Worktrees,
    /// Lifecycle markers on `agent:run` / `agent:queue` / `agent:loop` rows,
    /// from claims, release receipts, and completion evidence.
    Queue,
    /// `agent:todo`: operator checklist items, closed when their own tracked
    /// work id is durably resolved.
    Todo,
    /// `agent:backlog` (and the legacy `agent:pending` alias): open tracked work.
    Backlog,
    /// `agent:review`: gated tracked work.
    Review,
    /// `agent:icebox`: parked tracked work.
    Icebox,
    /// `agent:done`: the completed-work archive.
    Done,
}

impl ManagedList {
    /// Every managed list, in canonical component order.
    pub const ALL: [ManagedList; 8] = [
        ManagedList::Worktrees,
        ManagedList::Active,
        ManagedList::Queue,
        ManagedList::Review,
        ManagedList::Todo,
        ManagedList::Backlog,
        ManagedList::Icebox,
        ManagedList::Done,
    ];

    /// Canonical component name.
    pub fn component(self) -> &'static str {
        match self {
            ManagedList::Active => agent_doc_element::element::ACTIVE_COMPONENT,
            ManagedList::Worktrees => agent_doc_element::element::WORKTREES_COMPONENT,
            ManagedList::Queue => agent_doc_element::element::QUEUE_COMPONENT,
            ManagedList::Todo => "todo",
            ManagedList::Backlog => "backlog",
            ManagedList::Review => "review",
            ManagedList::Icebox => "icebox",
            ManagedList::Done => "done",
        }
    }

    /// Operator-facing name, e.g. `agent:todo`.
    pub fn label(self) -> &'static str {
        match self {
            ManagedList::Active => "agent:active",
            ManagedList::Worktrees => "agent:worktrees",
            ManagedList::Queue => "agent:run",
            ManagedList::Todo => "agent:todo",
            ManagedList::Backlog => "agent:backlog",
            ManagedList::Review => "agent:review",
            ManagedList::Icebox => "agent:icebox",
            ManagedList::Done => "agent:done",
        }
    }

    /// True when a parsed component named `name` belongs to this list.
    pub fn owns_component(self, name: &str) -> bool {
        let canonical = agent_doc_element::element::canonical_component_name(name);
        match self {
            ManagedList::Backlog => canonical == "backlog" || canonical == "pending",
            other => canonical == other.component(),
        }
    }
}

impl std::fmt::Display for ManagedList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The bodies of every component `list` owns, in document order.
pub fn component_bodies(content: &str, list: ManagedList) -> Result<Vec<&str>> {
    let components = agent_doc_element::element::parse(content)
        .with_context(|| format!("{list}: parse components"))?;
    Ok(components
        .iter()
        .filter(|component| list.owns_component(&component.name))
        .map(|component| component.content(content))
        .collect())
}

/// Replace the body of the first component `list` owns with `body` (full
/// component replace). Returns `None` when the document has no such component
/// or the body is already `body`.
pub fn replace_component_body(
    content: &str,
    list: ManagedList,
    body: &str,
) -> Result<Option<String>> {
    let components = agent_doc_element::element::parse(content)
        .with_context(|| format!("{list}: parse components"))?;
    let Some(component) = components
        .iter()
        .find(|component| list.owns_component(&component.name))
    else {
        return Ok(None);
    };
    if component.content(content) == body {
        return Ok(None);
    }
    Ok(Some(component.replace_content(content, body)))
}

/// The exact place where a rendered managed list differs from its
/// projection of durable state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMismatch {
    pub list: ManagedList,
    /// Which component occurrence of `list` (0-based) differs.
    pub occurrence: usize,
    /// 1-based line within the component body.
    pub line: usize,
    /// The line `render(durable_state)` produces there (`None`: past its end,
    /// or the component is missing from the projection).
    pub expected: Option<String>,
    /// The line the document shows there (`None`: past its end, or the
    /// component is missing from the document).
    pub observed: Option<String>,
}

impl std::fmt::Display for ListMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |line: &Option<String>| match line {
            Some(line) => format!("`{}`", bounded(line, 160)),
            None => "nothing".to_string(),
        };
        write!(
            f,
            "{}{} line {}: render(durable state) has {}, the document has {}",
            self.list,
            if self.occurrence > 0 {
                format!(" #{}", self.occurrence + 1)
            } else {
                String::new()
            },
            self.line,
            show(&self.expected),
            show(&self.observed),
        )
    }
}

fn bounded(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// The first differing line between two component bodies (1-based), or
/// `None` when they are byte-identical.
pub fn first_mismatch(
    expected: &str,
    observed: &str,
) -> Option<(usize, Option<String>, Option<String>)> {
    if expected == observed {
        return None;
    }
    let mut expected_lines = expected.split_inclusive('\n');
    let mut observed_lines = observed.split_inclusive('\n');
    let mut line = 0;
    loop {
        line += 1;
        match (expected_lines.next(), observed_lines.next()) {
            (None, None) => {
                // Same lines, different bytes: only possible through a
                // trailing-newline difference on the last line.
                return Some((line - 1, None, None));
            }
            (left, right) if left == right => continue,
            (left, right) => {
                let strip = |line: Option<&str>| {
                    line.map(|line| line.trim_end_matches(['\n', '\r']).to_string())
                };
                return Some((line, strip(left), strip(right)));
            }
        }
    }
}

/// Compare the `list` components of `observed_doc` with those of
/// `rendered_doc` and report the first difference.
pub fn list_mismatch(
    list: ManagedList,
    observed_doc: &str,
    rendered_doc: &str,
) -> Result<Option<ListMismatch>> {
    let observed = component_bodies(observed_doc, list)?;
    let rendered = component_bodies(rendered_doc, list)?;
    for occurrence in 0..observed.len().max(rendered.len()) {
        match (rendered.get(occurrence), observed.get(occurrence)) {
            (Some(expected), Some(seen)) => {
                if let Some((line, expected, observed)) = first_mismatch(expected, seen) {
                    return Ok(Some(ListMismatch {
                        list,
                        occurrence,
                        line,
                        expected,
                        observed,
                    }));
                }
            }
            (expected, seen) => {
                return Ok(Some(ListMismatch {
                    list,
                    occurrence,
                    line: 1,
                    expected: expected
                        .and_then(|body| body.lines().next())
                        .map(str::to_string),
                    observed: seen
                        .and_then(|body| body.lines().next())
                        .map(str::to_string),
                }));
            }
        }
    }
    Ok(None)
}

/// The `session-check` invariant for one list: `content`'s `list` component
/// equals `render(durable_state)`. `render` maps the document to its
/// reconciled successor and returns `None` at a fixed point (the convention
/// every `reconcile_*` projection in agent-doc follows).
pub fn check_list(
    list: ManagedList,
    content: &str,
    render: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<Option<ListMismatch>> {
    let Some(rendered) = render(content)? else {
        return Ok(None);
    };
    match list_mismatch(list, content, &rendered)? {
        Some(mismatch) => Ok(Some(mismatch)),
        // The successor differs only outside this list's components; the list
        // itself is in sync.
        None => Ok(None),
    }
}

/// Who wrote a row of a managed list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOwner {
    /// Written by the binary from a durable record (a lifecycle marker, an
    /// owner tag, an `agent:active` row). The reconcile may retire it when no
    /// record backs it.
    Binary,
    /// Typed by the operator. The reconcile may adopt it as a tracked record,
    /// move it verbatim, or report it — never delete or rewrite it.
    Operator,
}

/// One thing a reconcile pass did (or, in an audit, would do) to a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileAction {
    /// A binary-owned row (or binary-owned prefix of a row) with no durable
    /// record behind it was retired.
    RetireBinaryRow { list: ManagedList, text: String },
    /// A binary-owned marker with no durable record behind it was kept,
    /// because retiring it could re-dispatch finished work. Reported so the
    /// operator sees it; when in doubt, keep.
    KeepUnbacked { list: ManagedList, text: String },
    /// An operator-typed row with no durable record was adopted as a tracked
    /// item, text unchanged.
    AdoptOperatorRow { list: ManagedList, text: String },
    /// A durable record with no row was projected back into the list.
    Reproject { list: ManagedList, identity: String },
    /// A tracked item moved, verbatim, to the list its durable state names
    /// (for example a gated backlog item into `agent:review`).
    Migrate {
        id: String,
        from: ManagedList,
        to: ManagedList,
    },
    /// One stable id names more than one row. Reported, never merged.
    DuplicateId {
        list: ManagedList,
        id: String,
        rows: usize,
    },
}

impl std::fmt::Display for ReconcileAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReconcileAction::RetireBinaryRow { list, text } => {
                write!(
                    f,
                    "{list}: retired binary-owned `{}` (no durable record)",
                    bounded(text, 120)
                )
            }
            ReconcileAction::KeepUnbacked { list, text } => write!(
                f,
                "{list}: kept `{}` although no durable record backs it (retiring it could re-run finished work)",
                bounded(text, 120)
            ),
            ReconcileAction::AdoptOperatorRow { list, text } => {
                write!(f, "{list}: adopted operator row `{}`", bounded(text, 120))
            }
            ReconcileAction::Reproject { list, identity } => {
                write!(f, "{list}: re-projected durable record {identity}")
            }
            ReconcileAction::Migrate { id, from, to } => {
                write!(f, "moved [#{id}] from {from} to {to}")
            }
            ReconcileAction::DuplicateId { list, id, rows } => {
                write!(
                    f,
                    "{list}: [#{id}] names {rows} rows (reported, not merged)"
                )
            }
        }
    }
}

/// Operator-owned lines of `before` that `after` no longer contains, as a
/// multiset. `operator_text` maps a line to the operator-owned text it carries
/// (`None` for a binary-owned line, which a reconcile may change). A reconcile
/// that moves a line verbatim between components passes when both documents
/// are given whole.
///
/// This is the executable form of the hard invariant: operator-typed text is
/// never deleted or rewritten.
pub fn missing_operator_lines(
    before: &str,
    after: &str,
    operator_text: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut remaining: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in after.lines() {
        if let Some(text) = operator_text(line) {
            *remaining.entry(text).or_default() += 1;
        }
    }
    let mut missing = Vec::new();
    for line in before.lines() {
        let Some(text) = operator_text(line) else {
            continue;
        };
        match remaining.get_mut(&text) {
            Some(count) if *count > 0 => *count -= 1,
            _ => missing.push(text),
        }
    }
    missing
}

/// The projection laws every managed list's property tests assert.
pub mod laws {
    use std::fmt::Debug;

    /// A second render is byte-identical: `render(render(x)) == render(x)`.
    /// `render` returns `None` at a fixed point. Returns the fixed point.
    pub fn fixed_point(
        input: &str,
        render: impl Fn(&str) -> Option<String>,
    ) -> Result<String, String> {
        let once = render(input).unwrap_or_else(|| input.to_string());
        if let Some(twice) = render(&once)
            && twice != once
        {
            let (line, expected, observed) =
                super::first_mismatch(&once, &twice).unwrap_or((0, None, None));
            return Err(format!(
                "render is not idempotent: the second pass rewrote line {line} from {expected:?} to {observed:?}"
            ));
        }
        Ok(once)
    }

    /// Several renderings of the same durable state reach one fixed point,
    /// whatever their starting text. This is the oscillation check: two passes
    /// that each reach a fixed point of their own, but disagree, flip-flop
    /// forever when they alternate.
    pub fn one_fixed_point(
        starts: &[&str],
        render: impl Fn(&str) -> Option<String>,
    ) -> Result<String, String> {
        let mut agreed: Option<String> = None;
        for (index, start) in starts.iter().enumerate() {
            let fixed = fixed_point(start, &render)?;
            match &agreed {
                None => agreed = Some(fixed),
                Some(previous) if *previous == fixed => {}
                Some(previous) => {
                    let (line, expected, observed) =
                        super::first_mismatch(previous, &fixed).unwrap_or((0, None, None));
                    return Err(format!(
                        "start {index} reaches a different fixed point at line {line}: {expected:?} vs {observed:?}"
                    ));
                }
            }
        }
        agreed.ok_or_else(|| "no starting text".to_string())
    }

    /// For a sequence of lifecycle events, rendering after each event equals
    /// a from-scratch render of the same durable state, and a second render is
    /// a no-op.
    ///
    /// `apply` performs one lifecycle event on a state (the durable records
    /// plus any operator-owned text); `render` is the list's projection with
    /// full component replace. The incremental side renders after every event,
    /// the way the binary does inside each transition; the from-scratch side
    /// applies the same events without any intermediate render, then renders
    /// once. If they differ, the projection depends on its own earlier output.
    pub fn incremental_equals_from_scratch<S, E>(
        initial: &S,
        events: &[E],
        apply: impl Fn(&S, &E) -> S,
        render: impl Fn(&S) -> S,
    ) -> Result<S, String>
    where
        S: Clone + PartialEq + Debug,
        E: Debug,
    {
        let mut incremental = render(initial);
        let mut raw = initial.clone();
        for (index, event) in events.iter().enumerate() {
            incremental = render(&apply(&incremental, event));
            raw = apply(&raw, event);
            let scratch = render(&raw);
            if incremental != scratch {
                return Err(format!(
                    "after event {index} ({event:?}) the incremental render differs from a from-scratch render:\nincremental: {incremental:#?}\nscratch: {scratch:#?}"
                ));
            }
            let again = render(&incremental);
            if again != incremental {
                return Err(format!(
                    "after event {index} ({event:?}) a second render is not a no-op:\nonce: {incremental:#?}\ntwice: {again:#?}"
                ));
            }
        }
        Ok(incremental)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "## Run <!-- agent:run id=\"main\" -->\n",
        "- 📥 first\n",
        "<!-- /agent:run -->\n",
        "## Todo <!-- agent:todo -->\n",
        "- [ ] [#a] one\n",
        "- [ ] [#b] two\n",
        "<!-- /agent:todo -->\n",
        "## Backlog <!-- agent:pending -->\n",
        "- [ ] [#c] three\n",
        "<!-- /agent:pending -->\n",
    );

    #[test]
    fn lists_find_their_components_through_aliases() {
        assert_eq!(
            component_bodies(DOC, ManagedList::Queue).unwrap(),
            vec!["- 📥 first\n"]
        );
        assert_eq!(
            component_bodies(DOC, ManagedList::Backlog).unwrap(),
            vec!["- [ ] [#c] three\n"]
        );
        assert!(
            component_bodies(DOC, ManagedList::Active)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn full_component_replace_is_none_at_a_fixed_point() {
        let body = component_bodies(DOC, ManagedList::Todo).unwrap()[0].to_string();
        assert!(
            replace_component_body(DOC, ManagedList::Todo, &body)
                .unwrap()
                .is_none()
        );
        let replaced = replace_component_body(DOC, ManagedList::Todo, "- [x] [#a] one\n")
            .unwrap()
            .unwrap();
        assert!(replaced.contains("<!-- agent:todo -->\n- [x] [#a] one\n<!-- /agent:todo -->"));
        assert!(
            replaced.contains("- [ ] [#c] three\n"),
            "other components are untouched"
        );
    }

    #[test]
    fn mismatch_names_the_exact_line_of_the_component() {
        let render = |doc: &str| -> Result<Option<String>> {
            Ok(Some(doc.replace("- [ ] [#b] two", "- [x] [#b] two")))
        };
        let mismatch = check_list(ManagedList::Todo, DOC, render).unwrap().unwrap();
        assert_eq!(mismatch.line, 2);
        assert_eq!(mismatch.expected.as_deref(), Some("- [x] [#b] two"));
        assert_eq!(mismatch.observed.as_deref(), Some("- [ ] [#b] two"));
        assert_eq!(
            mismatch.to_string(),
            "agent:todo line 2: render(durable state) has `- [x] [#b] two`, the document has `- [ ] [#b] two`"
        );
        // A successor that changes only another list leaves this one in sync.
        assert!(
            check_list(ManagedList::Backlog, DOC, render)
                .unwrap()
                .is_none()
        );
        assert!(
            check_list(ManagedList::Todo, DOC, |_| Ok(None))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn mismatch_reports_extra_and_missing_lines() {
        assert_eq!(
            first_mismatch("a\nb\n", "a\n"),
            Some((2, Some("b".into()), None))
        );
        assert_eq!(
            first_mismatch("a\n", "a\nb\n"),
            Some((2, None, Some("b".into())))
        );
        assert_eq!(first_mismatch("a\n", "a\n"), None);
    }

    #[test]
    fn operator_lines_must_survive_a_reconcile_even_when_moved() {
        let operator = |line: &str| {
            let trimmed = line.trim();
            (trimmed.starts_with("- [")).then(|| trimmed.to_string())
        };
        let moved = DOC
            .replace("- [ ] [#c] three\n", "")
            .replace("- [ ] [#b] two\n", "- [ ] [#b] two\n- [ ] [#c] three\n");
        assert!(missing_operator_lines(DOC, &moved, operator).is_empty());
        let deleted = DOC.replace("- [ ] [#c] three\n", "");
        assert_eq!(
            missing_operator_lines(DOC, &deleted, operator),
            vec!["- [ ] [#c] three"]
        );
        let rewritten = DOC.replace("[#c] three", "[#c] 3");
        assert_eq!(missing_operator_lines(DOC, &rewritten, operator).len(), 1);
    }

    #[test]
    fn laws_reject_a_projection_that_reads_its_own_output() {
        // A toy list: the durable state is a count; the projection renders
        // that many rows. A faithful render recomputes from the count.
        #[derive(Debug, Clone, PartialEq)]
        struct State {
            count: usize,
            rendered: String,
        }
        let faithful = |state: &State| State {
            count: state.count,
            rendered: "- row\n".repeat(state.count),
        };
        let add = |state: &State, delta: &i32| State {
            count: (state.count as i32 + delta).max(0) as usize,
            rendered: state.rendered.clone(),
        };
        let start = State {
            count: 0,
            rendered: String::new(),
        };
        let events = [1, 1, -1, 2, -3];
        laws::incremental_equals_from_scratch(&start, &events, add, faithful).unwrap();

        // A sticky projection keeps rows it rendered before (it treats its own
        // earlier output as authority) — exactly the `☑️` ghost shape.
        let sticky = |state: &State| {
            let rows = state.count.max(state.rendered.lines().count());
            State {
                count: state.count,
                rendered: "- row\n".repeat(rows),
            }
        };
        let err = laws::incremental_equals_from_scratch(&start, &events, add, sticky).unwrap_err();
        assert!(err.contains("differs from a from-scratch render"), "{err}");
    }

    /// Regression shape for the `🙋` oscillation fixed in "operator-action
    /// projection reaches one fixed point" (8cca7e7940): two renderings of
    /// one answered waiting item each took a different gate branch, so passes
    /// alternately added and removed `🙋` every ~30 s. The shared law below
    /// fails without that fix.
    #[test]
    fn operator_action_projection_reaches_one_fixed_point() {
        let group = "- ### Waiting on you <!-- agent-doc:waiting-on-you -->\n";
        let subtree = concat!(
            "  - Before 2026-12-02T14:24:21Z, renew the sample runner license <!-- agent-doc:waiting-item --> [#tdrunnerlicense] ⟦effect:2⟧ [#subagent-operator-request-runner-license]\n",
            "    - **Response [#r-6b754fc3918c]**\n",
            "      - **Timestamp:** 2026-11-04T01:03:30.000Z\n",
            "      - **Summary:** **Deferred:** the operator renews the sample runner license.\n",
            "      - **State:** Waiting on operator input.\n",
            "      - **Guidance:** More work remains. Keep this tree and answer the operator request below.\n",
            "      - **Choices:**\n",
            "        - **Request**: [#subagent-operator-request-runner-license]\n",
            "        - **Renew it**: sign in before the deadline.\n",
            "    - **Response:** Before 2026-12-02T14:24:21Z, renew the sample runner license <!-- agent-doc:waiting-metadata -->\n",
            "      - **Question:** What decision or completion result should be recorded? <!-- agent-doc:waiting-metadata -->\n",
            "    - 📥 Complete the remaining work for the parent item.\n",
        );
        let bare = format!("{group}{subtree}");
        let glyph = bare.replacen("  - Before", "  - \u{1f64b} Before", 1);
        let queue_item = "Before 2026-12-02T14:24:21Z, renew the sample runner license <!-- agent-doc:waiting-item --> [#tdrunnerlicense] \u{27e6}effect:2\u{27e7}";
        let request = "subagent-operator-request-runner-license";
        let open =
            std::collections::HashSet::from(["tdrunnerlicense".to_string(), request.to_string()]);
        let with_action = vec![(queue_item.to_string(), request.to_string())];
        for actions in [with_action.as_slice(), &[]] {
            let render = |body: &str| {
                agent_doc_queue::document_queue::project_prompt_operator_actions(
                    body, actions, &open,
                )
                .unwrap()
                .map(|(projected, _)| projected)
            };
            laws::one_fixed_point(&[&bare, &glyph], render).unwrap();
        }
    }
}
