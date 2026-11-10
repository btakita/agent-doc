//! `#workerresponsestructure` — render a background worker's hand-back into
//! the response structure and extract its action items.
//!
//! `agent-doc queue report-complete` used to quote a worker's hand-back
//! verbatim. A hand-back the coordinator passed as one long line rendered as
//! one paragraph, so a limit the worker reported (an open item) was buried in
//! prose and never became queue work.
//!
//! A hand-back that labels its sections — `**Summary**`, `## Verification`,
//! `Action items:` and the aliases in [`LABELS`] — is parsed into
//! [`WorkerHandback`] and rendered in the canonical order
//! Blocker, Summary, What changed, Verification, Action items, Open items,
//! Notes, Next actions. Rendering
//! never drops text: every body line is kept byte for byte (outer blank lines
//! of a section are trimmed), an alias label survives as a sub-label inside
//! its canonical section, and text before the first label leads the Summary.
//! A hand-back that does not structure itself, meaning fewer than two labels
//! and no Blocker, Action items, or Open items label, is returned unchanged,
//! so an unstructured response is still recorded verbatim. A plain `Label:`
//! line counts only for a canonical heading; `Tests:` is ordinary prose.
//! `#blockerfront`: a Blocker section renders first even when the worker
//! wrote it last, and a Blocker label alone is structure so the reason is
//! never left buried in verbatim prose.
//!
//! `#handbackopenitemstriage`: the section a bullet sits in is the only signal
//! of what it asks for, because guessing from its prose is not reliable.
//! [`WorkerHandback::action_items`] lists work someone still has to do, and
//! the reporter turns each one into a follow-up queue row.
//! **Notes** (limits, state, FYIs) are rendered and never queued.
//! [`WorkerHandback::open_items`] lists the entries of the older **Open items**
//! section, which mixed the two: they are rendered and held for the
//! coordinator to triage, never queued unreviewed.

/// One canonical section of the response structure, in render order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionKind {
    /// Why the work stopped. Rendered first so a block is not buried (`#blockerfront`).
    Blocker,
    Summary,
    WhatChanged,
    Verification,
    /// Work someone still has to do: each entry becomes a queue row.
    ActionItems,
    /// Unclassified entries, held for coordinator triage.
    OpenItems,
    /// Limits, state, and FYIs: rendered, never queued.
    Notes,
    NextActions,
}

impl SectionKind {
    pub const ALL: [SectionKind; 8] = [
        SectionKind::Blocker,
        SectionKind::Summary,
        SectionKind::WhatChanged,
        SectionKind::Verification,
        SectionKind::ActionItems,
        SectionKind::OpenItems,
        SectionKind::Notes,
        SectionKind::NextActions,
    ];

    /// The rendered section heading.
    pub fn heading(self) -> &'static str {
        match self {
            SectionKind::Blocker => "Blocker",
            SectionKind::Summary => "Summary",
            SectionKind::WhatChanged => "What changed",
            SectionKind::Verification => "Verification",
            SectionKind::ActionItems => "Action items",
            SectionKind::OpenItems => "Open items",
            SectionKind::Notes => "Notes",
            SectionKind::NextActions => "Next actions",
        }
    }
}

/// Recognized section labels (lowercase) and the canonical section each one
/// belongs to. A label that is not the canonical heading survives as a
/// sub-label inside its section.
pub const LABELS: &[(&str, SectionKind)] = &[
    ("blocker", SectionKind::Blocker),
    ("blockers", SectionKind::Blocker),
    ("summary", SectionKind::Summary),
    ("what changed", SectionKind::WhatChanged),
    ("changes", SectionKind::WhatChanged),
    ("branch + commits", SectionKind::WhatChanged),
    ("branch and commits", SectionKind::WhatChanged),
    ("commits", SectionKind::WhatChanged),
    ("files changed", SectionKind::WhatChanged),
    ("verification", SectionKind::Verification),
    ("tests", SectionKind::Verification),
    ("test results", SectionKind::Verification),
    ("action items", SectionKind::ActionItems),
    ("action item", SectionKind::ActionItems),
    ("remaining work", SectionKind::ActionItems),
    ("open items", SectionKind::OpenItems),
    ("open item", SectionKind::OpenItems),
    ("open questions", SectionKind::OpenItems),
    ("notes", SectionKind::Notes),
    ("limits", SectionKind::Notes),
    ("known limits", SectionKind::Notes),
    ("limitations", SectionKind::Notes),
    ("next actions", SectionKind::NextActions),
    ("next action", SectionKind::NextActions),
    ("next steps", SectionKind::NextActions),
    ("follow-ups", SectionKind::NextActions),
    ("follow ups", SectionKind::NextActions),
    ("followups", SectionKind::NextActions),
];

/// A labelled section as the worker wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandbackSection {
    pub kind: SectionKind,
    /// The label exactly as written (without bold/heading/colon syntax).
    pub label: String,
    /// Body lines, outer blank lines trimmed. Text that followed the label on
    /// the label's own line is the first body line.
    pub lines: Vec<String>,
}

/// A hand-back parsed into labelled sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerHandback {
    /// Text before the first label (outer blank lines trimmed).
    pub preamble: Vec<String>,
    /// Sections in source order.
    pub sections: Vec<HandbackSection>,
}

/// Values of an item section that mean "nothing is open".
const NONE_MARKERS: &[&str] = &[
    "none",
    "n/a",
    "na",
    "nothing",
    "no open items",
    "nothing open",
    "nothing left",
    "nothing remaining",
    "none known",
    "-",
    "[]",
];

impl WorkerHandback {
    /// Parse `text`, or `None` when it does not structure itself (record it
    /// verbatim).
    pub fn parse(text: &str) -> Option<Self> {
        let mut preamble = Vec::new();
        let mut sections: Vec<HandbackSection> = Vec::new();
        let mut fence: Option<String> = None;
        for raw in text.lines() {
            let line = raw.trim_end();
            if let Some(marker) = &fence {
                if line.trim_start().starts_with(marker.as_str()) {
                    fence = None;
                }
                push_line(&mut preamble, &mut sections, line.to_string());
                continue;
            }
            if let Some(marker) = fence_marker(line) {
                fence = Some(marker);
                push_line(&mut preamble, &mut sections, line.to_string());
                continue;
            }
            for segment in split_inline_labels(line) {
                match parse_label_line(&segment) {
                    Some((kind, label, rest)) => {
                        let mut lines = Vec::new();
                        if !rest.is_empty() {
                            lines.push(rest);
                        }
                        sections.push(HandbackSection { kind, label, lines });
                    }
                    None => push_line(&mut preamble, &mut sections, segment),
                }
            }
        }
        // One stray label in prose (`Tests: 12 passed.`) is not a structure.
        // A Blocker, Action items, or Open items label alone still is: those
        // sections must not be buried.
        let has_item_section = sections.iter().any(|section| {
            matches!(
                section.kind,
                SectionKind::Blocker | SectionKind::ActionItems | SectionKind::OpenItems
            )
        });
        if sections.len() < 2 && !has_item_section {
            return None;
        }
        trim_blank_edges(&mut preamble);
        for section in &mut sections {
            trim_blank_edges(&mut section.lines);
        }
        Some(Self { preamble, sections })
    }

    /// Render the response structure: each present canonical section once,
    /// in canonical order, as `**Heading**`, a blank line, then its body.
    pub fn render(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();
        let has_summary = self
            .sections
            .iter()
            .any(|section| section.kind == SectionKind::Summary);
        if has_summary && !self.preamble.is_empty() {
            blocks.push(self.preamble.join("\n"));
        }
        for kind in SectionKind::ALL {
            let mut parts: Vec<String> = Vec::new();
            if kind == SectionKind::Summary && !has_summary && !self.preamble.is_empty() {
                parts.push(self.preamble.join("\n"));
            }
            for section in self.sections.iter().filter(|section| section.kind == kind) {
                let is_canonical = section.label.eq_ignore_ascii_case(kind.heading());
                if is_canonical {
                    if !section.lines.is_empty() {
                        parts.push(section.lines.join("\n"));
                    }
                } else if section.lines.is_empty() {
                    parts.push(format!("_{}:_", section.label));
                } else {
                    let mut lines = section.lines.clone();
                    if starts_list_or_block(&lines[0]) {
                        lines.insert(0, format!("_{}:_", section.label));
                    } else {
                        lines[0] = format!("_{}:_ {}", section.label, lines[0]);
                    }
                    parts.push(lines.join("\n"));
                }
            }
            if parts.is_empty() {
                continue;
            }
            blocks.push(format!("**{}**\n\n{}", kind.heading(), parts.join("\n\n")));
        }
        blocks.join("\n\n")
    }

    /// Every action item, which is the only entry that becomes a queue row.
    pub fn action_items(&self) -> Vec<String> {
        self.items_of(SectionKind::ActionItems)
    }

    /// Every entry of an Open items section, held for coordinator triage.
    pub fn open_items(&self) -> Vec<String> {
        self.items_of(SectionKind::OpenItems)
    }

    /// The entries of every `kind` section, one whitespace-collapsed line
    /// each, in order and de-duplicated. A "none"-style value lists nothing.
    fn items_of(&self, kind: SectionKind) -> Vec<String> {
        let mut items: Vec<String> = Vec::new();
        for section in &self.sections {
            if section.kind != kind {
                continue;
            }
            for item in split_items(&section.lines) {
                if is_none_marker(&item) {
                    continue;
                }
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        items
    }
}

/// The response text a completion cell records: the rendered structure when
/// `response` labels its sections, else `response` unchanged.
pub fn structure_response(response: &str) -> String {
    match WorkerHandback::parse(response) {
        Some(handback) => handback.render(),
        None => response.to_string(),
    }
}

/// The action items of `response` (empty when it labels no Action items
/// section).
pub fn action_items(response: &str) -> Vec<String> {
    WorkerHandback::parse(response)
        .map(|handback| handback.action_items())
        .unwrap_or_default()
}

/// The open items of `response` (empty when it labels no Open items section).
pub fn open_items(response: &str) -> Vec<String> {
    WorkerHandback::parse(response)
        .map(|handback| handback.open_items())
        .unwrap_or_default()
}

fn push_line(preamble: &mut Vec<String>, sections: &mut [HandbackSection], line: String) {
    match sections.last_mut() {
        Some(section) => section.lines.push(line),
        None => preamble.push(line),
    }
}

fn trim_blank_edges(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let leading = lines
        .iter()
        .take_while(|line| line.trim().is_empty())
        .count();
    lines.drain(..leading);
}

fn fence_marker(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    for marker in ['`', '~'] {
        let run = trimmed.chars().take_while(|ch| *ch == marker).count();
        if run >= 3 {
            return Some(marker.to_string().repeat(run));
        }
    }
    None
}

fn label_kind(label: &str) -> Option<SectionKind> {
    let normalized = label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    LABELS
        .iter()
        .find(|(name, _)| *name == normalized)
        .map(|(_, kind)| *kind)
}

/// Split a line before every mid-line bold label that ends in a colon
/// (`… **Open items:** …` or `… **Open items**: …`), so a hand-back flattened
/// onto one line still separates. A segment never splits inside a code span.
fn split_inline_labels(line: &str) -> Vec<String> {
    let mut cuts = Vec::new();
    let mut in_code = false;
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'`' => {
                in_code = !in_code;
                index += 1;
            }
            b'*' if !in_code
                && index > 0
                && bytes.get(index + 1) == Some(&b'*')
                && bytes[index - 1].is_ascii_whitespace() =>
            {
                if let Some((label, after)) = bold_label(&line[index..])
                    && label_kind(label.trim_end_matches(':')).is_some()
                    && (label.ends_with(':') || after.starts_with(':'))
                {
                    cuts.push(index);
                }
                index += 2;
            }
            _ => index += 1,
        }
    }
    if cuts.is_empty() {
        return vec![line.to_string()];
    }
    let mut segments = Vec::new();
    let mut start = 0;
    for cut in cuts {
        let segment = line[start..cut].trim_end();
        if !segment.trim().is_empty() {
            segments.push(segment.to_string());
        }
        start = cut;
    }
    segments.push(line[start..].to_string());
    segments
}

/// `**label**rest` → (`label`, `rest`).
fn bold_label(text: &str) -> Option<(&str, &str)> {
    let inner = text.strip_prefix("**")?;
    let end = inner.find("**")?;
    let label = &inner[..end];
    if label.is_empty() || label.trim() != label {
        return None;
    }
    Some((label, &inner[end + 2..]))
}

/// Recognize a section label line: a heading (`## Summary`), a bold label
/// (`**Summary**`, `**Summary:** text`, `- **Open items:** text`), or a plain
/// `Label:` at the start of the line. Returns the kind, the label as written,
/// and the text that followed it on the line.
fn parse_label_line(line: &str) -> Option<(SectionKind, String, String)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('#') {
        let heading = rest.trim_start_matches('#');
        let level = trimmed.len() - heading.len();
        if level <= 6 && heading.starts_with(' ') {
            let text = heading.trim().trim_end_matches(':').trim();
            let text = text
                .strip_prefix("**")
                .and_then(|text| text.strip_suffix("**"))
                .unwrap_or(text);
            let text = text.trim_end_matches(':').trim();
            return label_kind(text).map(|kind| (kind, text.to_string(), String::new()));
        }
        return None;
    }
    let body = ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| trimmed.strip_prefix(bullet))
        .unwrap_or(trimmed);
    if let Some((label, after)) = bold_label(body) {
        let name = label.trim_end_matches(':').trim();
        let kind = label_kind(name)?;
        let after = after.strip_prefix(':').unwrap_or(after);
        return Some((kind, name.to_string(), after.trim().to_string()));
    }
    // Plain `Label:` only at the very start of an unindented, unbulleted line,
    // and only for a canonical heading: `Tests:` or `Changes:` is ordinary
    // prose in an unstructured hand-back.
    if body.len() != trimmed.len() || trimmed.len() != line.len() {
        return None;
    }
    let (label, after) = line.split_once(':')?;
    let kind = label_kind(label)?;
    if !label.trim().eq_ignore_ascii_case(kind.heading()) {
        return None;
    }
    if !(after.is_empty() || after.starts_with(' ')) {
        return None;
    }
    Some((kind, label.trim().to_string(), after.trim().to_string()))
}

fn starts_list_or_block(line: &str) -> bool {
    let trimmed = line.trim_start();
    list_item_body(trimmed).is_some()
        || trimmed.starts_with('>')
        || trimmed.starts_with('|')
        || fence_marker(line).is_some()
}

/// The text after a list marker (`- `, `* `, `+ `, `1. `, `1) `).
fn list_item_body(trimmed: &str) -> Option<&str> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            return Some(rest);
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 9 {
        let rest = &trimmed[digits..];
        for marker in [". ", ") "] {
            if let Some(rest) = rest.strip_prefix(marker) {
                return Some(rest);
            }
        }
    }
    None
}

/// Top-level list entries (with their continuation and nested lines) when the
/// section is a list, else blank-line-separated paragraphs. Each item is one
/// whitespace-collapsed line.
fn split_items(lines: &[String]) -> Vec<String> {
    let base_indent = lines
        .iter()
        .filter(|line| list_item_body(line.trim_start()).is_some())
        .map(|line| line.len() - line.trim_start().len())
        .min();
    let mut items: Vec<Vec<String>> = Vec::new();
    match base_indent {
        Some(base) => {
            let mut leading: Vec<String> = Vec::new();
            for line in lines {
                let trimmed = line.trim_start();
                let indent = line.len() - trimmed.len();
                if indent == base
                    && let Some(body) = list_item_body(trimmed)
                {
                    items.push(vec![body.to_string()]);
                    continue;
                }
                if trimmed.is_empty() {
                    continue;
                }
                let text = list_item_body(trimmed).unwrap_or(trimmed).to_string();
                match items.last_mut() {
                    Some(item) => item.push(text),
                    None => leading.push(text),
                }
            }
            if !leading.is_empty() {
                items.insert(0, leading);
            }
        }
        None => {
            let mut paragraph: Vec<String> = Vec::new();
            for line in lines {
                if line.trim().is_empty() {
                    if !paragraph.is_empty() {
                        items.push(std::mem::take(&mut paragraph));
                    }
                } else {
                    paragraph.push(line.trim().to_string());
                }
            }
            if !paragraph.is_empty() {
                items.push(paragraph);
            }
        }
    }
    items
        .into_iter()
        .map(|item| {
            item.join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn is_none_marker(item: &str) -> bool {
    let mut normalized = item.trim();
    loop {
        let next = normalized
            .trim_matches(['*', '_', '`'])
            .trim_end_matches(['.', '!'])
            .trim();
        if next == normalized {
            break;
        }
        normalized = next;
    }
    let normalized = normalized.to_ascii_lowercase();
    NONE_MARKERS.contains(&normalized.as_str())
        || normalized.starts_with("none ")
        || normalized.starts_with("none.")
        || normalized.starts_with("none;")
        || normalized.starts_with("none,")
        || normalized.starts_with("none (")
        || normalized.starts_with("none —")
        || normalized.starts_with("none -")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word of the hand-back, ignoring the label syntax the renderer
    /// rewrites, so a test can prove rendering lost no text.
    fn content_words(text: &str) -> Vec<String> {
        let mut words: Vec<String> = text
            .split_whitespace()
            .map(|word| word.trim_matches(['*', '_', ':', '#']).to_ascii_lowercase())
            .filter(|word| !word.is_empty())
            .collect();
        words.sort();
        words
    }

    fn label_words() -> Vec<String> {
        SectionKind::ALL
            .iter()
            .flat_map(|kind| kind.heading().split_whitespace())
            .map(str::to_ascii_lowercase)
            .collect()
    }

    /// Words of `output` minus one occurrence of each canonical heading word
    /// per rendered heading; must equal the input's words minus its labels.
    fn assert_no_text_lost(input: &str, output: &str) {
        let input_words = content_words(input);
        let mut output_words = content_words(output);
        for word in &input_words {
            let Some(position) = output_words.iter().position(|candidate| candidate == word) else {
                panic!("rendering lost {word:?}\ninput:\n{input}\noutput:\n{output}");
            };
            output_words.remove(position);
        }
        let headings = label_words();
        for word in output_words {
            assert!(
                headings.contains(&word),
                "rendering invented {word:?}\noutput:\n{output}"
            );
        }
    }

    #[test]
    fn unlabelled_handback_is_recorded_verbatim() {
        let text = "Implemented the requested change.\n\n- focused tests passed\n- branch pushed";
        assert_eq!(WorkerHandback::parse(text), None);
        assert_eq!(structure_response(text), text);
        assert!(open_items(text).is_empty());
    }

    /// A prose hand-back with one label-shaped line, or with plain alias
    /// lines, is not a structured hand-back: `Tests: 12 passed.` stays exactly
    /// where the worker wrote it.
    #[test]
    fn one_stray_label_or_plain_aliases_stay_verbatim() {
        for text in [
            "Implemented SDK support.\n\nTests: 12 passed.",
            "Implemented it.\n\nVerification: make check exit 0.",
            "**Summary:** a single bold label",
            "Changes: one file\nTests: 12 passed\nLimits: none\nNext steps: ship",
        ] {
            assert_eq!(structure_response(text), text, "{text:?}");
            assert!(open_items(text).is_empty(), "{text:?}");
        }
    }

    /// `#blockerfront`: the blocker is the first section of the response
    /// structure even when the hand-back wrote Summary above it.
    #[test]
    fn a_blocker_section_renders_before_summary() {
        let text = concat!(
            "**Summary**\n\nThe row is blocked.\n\n",
            "**Blocker**\n\nThe harness usage limit stopped the worker.\n",
        );
        let rendered = structure_response(text);
        let blocker = rendered.find("**Blocker**").expect("blocker section");
        let summary = rendered.find("**Summary**").expect("summary section");
        assert!(blocker < summary, "{rendered}");
        assert!(rendered.contains("The harness usage limit stopped the worker."));
        assert_no_text_lost(text, &rendered);
    }

    /// A lone Blocker is structure. Preamble becomes Summary after it, so the
    /// reason stays in front instead of being recorded as verbatim prose.
    #[test]
    fn a_lone_blocker_keeps_the_preamble_as_summary_after_it() {
        let text = "Context before the label.\n\n**Blocker**\n\nquota exhausted\n";
        let rendered = structure_response(text);
        assert!(
            rendered.starts_with("**Blocker**\n\nquota exhausted"),
            "{rendered}"
        );
        let summary = rendered.find("**Summary**").expect("summary");
        assert!(
            rendered[summary..].contains("Context before the label."),
            "{rendered}"
        );
    }

    #[test]
    fn an_open_items_label_alone_is_enough_to_surface_the_item() {
        let text = "All done.\n\nOpen items: the timer test is still flaky";
        assert_eq!(
            structure_response(text),
            "**Summary**\n\nAll done.\n\n**Open items**\n\nthe timer test is still flaky"
        );
        assert_eq!(
            open_items(text),
            vec!["the timer test is still flaky".to_string()]
        );
    }

    #[test]
    fn bold_labelled_handback_renders_in_canonical_order() {
        let text = concat!(
            "**Summary** Stamped the decision id onto todo copies.\n",
            "**Branch + commits**\n",
            "- fix/decisionidstamp-20261105 at 12f8e5ac79 (pushed)\n",
            "**Files changed**\n",
            "- agent-doc-element/src/todo.rs\n",
            "**Verification**\n",
            "`make check` exit=0; nextest 4211 passed, 0 failed\n",
            "**Open items**\n",
            "- >512-commit documents can miss answers\n",
            "- the editor gutter does not show the stamp\n",
            "  yet (needs a plugin release)\n",
            "**Follow-ups**\n",
            "- do [#stampgutter] show decision stamps in the gutter\n",
        );
        let handback = WorkerHandback::parse(text).expect("labelled");
        let rendered = handback.render();
        assert_eq!(
            rendered,
            concat!(
                "**Summary**\n\nStamped the decision id onto todo copies.\n\n",
                "**What changed**\n\n",
                "_Branch + commits:_\n- fix/decisionidstamp-20261105 at 12f8e5ac79 (pushed)\n\n",
                "_Files changed:_\n- agent-doc-element/src/todo.rs\n\n",
                "**Verification**\n\n`make check` exit=0; nextest 4211 passed, 0 failed\n\n",
                "**Open items**\n\n- >512-commit documents can miss answers\n",
                "- the editor gutter does not show the stamp\n  yet (needs a plugin release)\n\n",
                "**Next actions**\n\n",
                "_Follow-ups:_\n- do [#stampgutter] show decision stamps in the gutter",
            )
        );
        assert_no_text_lost(text, &rendered);
        assert_eq!(
            handback.open_items(),
            vec![
                ">512-commit documents can miss answers".to_string(),
                "the editor gutter does not show the stamp yet (needs a plugin release)"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn heading_form_and_plain_labels_are_recognized() {
        let text = concat!(
            "Worker notes first.\n\n",
            "## Verification\n\ncargo test -p agent-doc-queue: 12 passed\n\n",
            "### What changed:\n\nOne file.\n\n",
            "Open items: the flaky timer test is still open\n",
            "Next actions: none\n",
        );
        let handback = WorkerHandback::parse(text).expect("labelled");
        let rendered = handback.render();
        assert_eq!(
            rendered,
            concat!(
                "**Summary**\n\nWorker notes first.\n\n",
                "**What changed**\n\nOne file.\n\n",
                "**Verification**\n\ncargo test -p agent-doc-queue: 12 passed\n\n",
                "**Open items**\n\nthe flaky timer test is still open\n\n",
                "**Next actions**\n\nnone",
            )
        );
        assert_no_text_lost(text, &rendered);
        assert_eq!(
            handback.open_items(),
            vec!["the flaky timer test is still open".to_string()]
        );
    }

    #[test]
    fn a_flattened_one_line_handback_still_separates_its_sections() {
        let text = "decisionidstamp: branch pushed. **Verification:** make check exit 0. **Open items:** >512-commit documents can miss answers. **Next actions:** none.";
        let handback = WorkerHandback::parse(text).expect("labelled");
        assert_eq!(
            handback.render(),
            concat!(
                "**Summary**\n\ndecisionidstamp: branch pushed.\n\n",
                "**Verification**\n\nmake check exit 0.\n\n",
                "**Open items**\n\n>512-commit documents can miss answers.\n\n",
                "**Next actions**\n\nnone.",
            )
        );
        assert_no_text_lost(text, &handback.render());
        assert_eq!(
            handback.open_items(),
            vec![">512-commit documents can miss answers.".to_string()]
        );
    }

    #[test]
    fn none_open_items_queue_nothing_and_paragraphs_are_items() {
        for none in [
            "None.",
            "none",
            "- none",
            "N/A",
            "_None._",
            "None — all done.",
        ] {
            let text = format!("**Summary:** done\n**Open items:**\n{none}\n");
            assert!(open_items(&text).is_empty(), "{none:?} queued an item");
        }
        let text = "**Summary:** done\n**Open items:**\nfirst limit\nwraps here\n\nsecond limit\n";
        assert_eq!(
            open_items(text),
            vec![
                "first limit wraps here".to_string(),
                "second limit".to_string()
            ]
        );
    }

    #[test]
    fn labels_inside_code_fences_and_code_spans_and_prose_are_not_sections() {
        let text = concat!(
            "**Summary:** reproduced it.\n",
            "**Verification:** reran it.\n",
            "```\n",
            "**Open items:** inside a fence\n",
            "Verification: inside a fence\n",
            "```\n",
            "The word summary: in prose is not a label, nor `x **Open items:** y`.\n",
        );
        let handback = WorkerHandback::parse(text).expect("labelled");
        assert_eq!(handback.sections.len(), 2);
        assert!(handback.open_items().is_empty());
        assert_no_text_lost(text, &handback.render());
    }

    #[test]
    fn duplicate_open_items_are_listed_once() {
        let text = "**Open items:**\n- same limit\n- same limit\n**Open questions:**\n- same limit\n- other\n";
        assert_eq!(
            open_items(text),
            vec!["same limit".to_string(), "other".to_string()]
        );
    }

    #[test]
    fn rendering_is_a_fixed_point() {
        let text = concat!(
            "**Summary:** a\n**Verification:** b\n**Action items:**\n- c\n",
            "**Open items:**\n- d\n**Notes:**\n- e\n",
        );
        let once = structure_response(text);
        assert_eq!(structure_response(&once), once);
        assert_eq!(action_items(&once), action_items(text));
        assert_eq!(open_items(&once), open_items(text));
    }

    /// `#handbackopenitemstriage`: the section decides what a bullet asks for.
    /// Only an Action items entry is work; notes are kept for the reader, and
    /// an Open items entry stays unclassified.
    #[test]
    fn action_items_notes_and_open_items_stay_apart() {
        let text = concat!(
            "**Summary:** done\n",
            "**Notes:**\n- `cargo clean` is done\n",
            "**Remaining work:**\n- wire the gutter stamp\n",
            "**Limits:**\n- >512-commit documents can miss answers\n",
            "**Open items:**\n- Nothing was applied to it\n",
            "**Action items:**\n- file the anchor as a backlog item\n",
        );
        let handback = WorkerHandback::parse(text).expect("labelled");
        assert_eq!(
            handback.action_items(),
            vec![
                "wire the gutter stamp".to_string(),
                "file the anchor as a backlog item".to_string(),
            ]
        );
        assert_eq!(
            handback.open_items(),
            vec!["Nothing was applied to it".to_string()]
        );
        let rendered = handback.render();
        assert_no_text_lost(text, &rendered);
        let position = |needle: &str| {
            rendered
                .find(needle)
                .unwrap_or_else(|| panic!("{needle:?} missing: {rendered}"))
        };
        assert!(position("**Action items**") < position("**Open items**"));
        assert!(position("**Open items**") < position("**Notes**"));
        assert!(position("`cargo clean` is done") > position("**Notes**"));
        assert!(position("_Limits:_") > position("**Notes**"));
    }

    #[test]
    fn an_action_items_label_alone_is_a_structure_and_a_notes_label_alone_is_not() {
        let text = "All done.\n\nAction items: fix the timer test";
        assert_eq!(
            structure_response(text),
            "**Summary**\n\nAll done.\n\n**Action items**\n\nfix the timer test"
        );
        assert_eq!(action_items(text), vec!["fix the timer test".to_string()]);

        let note = "All done.\n\nNotes: `cargo clean` is done";
        assert_eq!(structure_response(note), note);
        assert!(action_items(note).is_empty() && open_items(note).is_empty());
    }

    #[test]
    fn none_action_items_queue_nothing() {
        for none in ["None.", "none", "- none", "N/A"] {
            let text = format!("**Summary:** done\n**Action items:**\n{none}\n");
            assert!(action_items(&text).is_empty(), "{none:?} queued an item");
        }
    }
}
