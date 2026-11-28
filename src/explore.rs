//! Terminal host for the shared Lazily View-backed Document Explorer.

mod highlight;

use crate::document_explorer::{
    PromptFailure, explorer_active_prompts, load_open_cycle, persist_prompt, spawn_prompt_run,
};
use agent_doc_editor_surface_io::{DocumentPromptSubmission, project_document_prompts_with_active};
use anyhow::{Context, Result, ensure};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{
    self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use highlight::{Highlights, SourcePreview};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;
use unicode_width::UnicodeWidthChar;

const SOURCE: &str = "terminal_document_explorer";

pub(crate) fn run(file: &Path) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "agent-doc explore requires an interactive terminal"
    );
    let file = file
        .canonicalize()
        .with_context(|| format!("open {}", file.display()))?;
    ensure!(file.is_file(), "expected a document file");
    let mut explorer = Explorer::open(file)?;
    let mut screen = Terminal::enter()?;
    explorer.interact(&mut screen.output)
}

#[derive(Deserialize)]
struct Projection {
    revision: String,
    nodes: Vec<Row>,
    view: View,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    selection_key: Option<String>,
    label: String,
    line: usize,
    end_line: Option<usize>,
    depth: usize,
    action: String,
    task: Option<Task>,
    #[serde(skip)]
    highlights: Highlights,
}

impl Row {
    fn render(&self, output: &mut impl Write, y: u16, width: u16, active: bool) -> io::Result<()> {
        let indent = self.depth.saturating_mul(2).min(usize::from(width / 2));
        let prefix = format!("{}{}", if active { "› " } else { "  " }, " ".repeat(indent));
        let mut columns = usize::from(width);
        let visible_prefix = clip_to_width(&prefix, &mut columns);
        if active {
            queue!(output, SetAttribute(Attribute::Reverse))?;
        }
        queue!(output, MoveTo(0, y), Print(visible_prefix))?;
        if visible_prefix.len() == prefix.len() {
            let label_len = clip_to_width(&self.label, &mut columns).len();
            highlight::print_spans(output, &self.label, 0..label_len, &self.highlights, active)?;
            if label_len == self.label.len()
                && let Some(task) = &self.task
            {
                let badge = format!(" [{}]", task.label);
                let visible_badge: String =
                    printable(clip_to_width(&badge, &mut columns)).collect();
                queue!(output, Print(visible_badge))?;
            }
        }
        if active {
            queue!(output, SetAttribute(Attribute::Reset))?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Task {
    label: String,
}

#[derive(Deserialize)]
struct View {
    title: String,
    prompt_label: String,
    prompt_actions: HashMap<String, PromptAction>,
}

#[derive(Deserialize)]
struct PromptAction {
    label: String,
    hint: String,
    disabled: bool,
}

struct Snapshot {
    preview: SourcePreview,
    projection: Projection,
}

impl Snapshot {
    fn load(file: &Path) -> Result<Self> {
        let content = agent_doc_document_realtime_io::peek_current_document_content(file, SOURCE)
            .context("read authoritative document")?;
        Self::project(file, content)
    }

    fn project(file: &Path, content: String) -> Result<Self> {
        let cycle = load_open_cycle(file).context("read active prompt state")?;
        let active_prompts = cycle
            .iter()
            .flat_map(explorer_active_prompts)
            .cloned()
            .collect::<Vec<_>>();
        let mut projection: Projection = serde_json::from_value(
            project_document_prompts_with_active(&content, &active_prompts)?,
        )
        .context("decode the Document Explorer view")?;
        let colored = highlight::enabled();
        let preview = SourcePreview::new(content, colored);
        if colored {
            for row in &mut projection.nodes {
                if row.id != "document" {
                    row.highlights =
                        preview.outline_highlights(&row.label, row.line.saturating_sub(1));
                }
            }
        }
        Ok(Self {
            preview,
            projection,
        })
    }
}

#[derive(PartialEq)]
enum DraftTarget {
    Stable(String),
    Snapshot { revision: String, id: String },
}

struct DraftAttempt {
    receipt: String,
    target: DraftTarget,
    text: String,
}

impl DraftAttempt {
    fn for_submission(previous: Option<Self>, target: DraftTarget, text: &str) -> Self {
        match previous {
            Some(attempt) if attempt.target == target && attempt.text == text => attempt,
            _ => Self {
                receipt: uuid::Uuid::new_v4().simple().to_string(),
                target,
                text: text.into(),
            },
        }
    }
}

#[derive(Clone, Copy, Default)]
enum Focus {
    #[default]
    Outline,
    Preview,
}

#[derive(Default)]
struct Search {
    input: Option<String>,
    query: String,
    preview_match: Option<usize>,
}

fn next_match(
    count: usize,
    current: usize,
    backward: bool,
    matches: impl FnMut(&usize) -> bool,
) -> Option<usize> {
    let current = current.min(count.checked_sub(1)?);
    if backward {
        (0..current)
            .rev()
            .chain((current..count).rev())
            .find(matches)
    } else {
        (current + 1..count).chain(0..=current).find(matches)
    }
}

struct Explorer {
    file: PathBuf,
    snapshot: Snapshot,
    selected: usize,
    source_scroll: usize,
    focus: Focus,
    search: Search,
    pane_sizes: PaneSizes,
    status: String,
    composing: bool,
    draft: String,
    draft_attempt: Option<DraftAttempt>,
    running: Option<Receiver<Result<std::process::ExitStatus, io::Error>>>,
}

impl Explorer {
    fn open(file: PathBuf) -> Result<Self> {
        let snapshot = Snapshot::load(&file)?;
        Ok(Self::new(file, snapshot))
    }

    fn new(file: PathBuf, snapshot: Snapshot) -> Self {
        Self {
            file,
            snapshot,
            selected: 0,
            source_scroll: 0,
            focus: Focus::default(),
            search: Search::default(),
            pane_sizes: PaneSizes::default(),
            status: String::new(),
            composing: false,
            draft: String::new(),
            draft_attempt: None,
            running: None,
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.snapshot.projection.nodes.get(self.selected)
    }

    fn selected_action(&self) -> Option<&PromptAction> {
        let row = self.selected_row()?;
        self.snapshot
            .projection
            .view
            .prompt_actions
            .get(&row.action)
    }

    fn interact(&mut self, output: &mut impl Write) -> Result<()> {
        loop {
            self.observe_run();
            let (width, height) = terminal::size()?;
            self.render(output, width, height)?;
            // The dispatch receipt is sent by a background worker. Wait on
            // input outright when no other event source needs servicing.
            while self.running.is_some() && !event::poll(Duration::from_millis(150))? {
                self.observe_run();
                if self.running.is_none() {
                    self.render(output, width, height)?;
                }
            }
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if self.key(key, width, height) {
                        return Ok(());
                    }
                }
                Event::Paste(text) if self.composing => {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    self.draft.extend(
                        text.chars()
                            .filter(|c| matches!(c, '\n' | '\t') || !c.is_control()),
                    );
                }
                Event::Paste(text) => {
                    if let Some(input) = &mut self.search.input {
                        input.extend(text.chars().filter(|c| !c.is_control()));
                    }
                }
                _ => {}
            }
        }
    }

    fn key(&mut self, key: KeyEvent, width: u16, height: u16) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            return true;
        }
        if self.composing {
            match key.code {
                KeyCode::Esc => self.composing = false,
                KeyCode::Char('s') if control => self.submit(),
                KeyCode::Char('r') if control => self.refresh(),
                KeyCode::Enter => self.draft.push('\n'),
                KeyCode::Backspace => {
                    self.draft.pop();
                }
                KeyCode::Char(c) if !control && !c.is_control() => self.draft.push(c),
                _ => {}
            }
            return false;
        }

        let panes = self.pane_sizes.fit(width, height);
        if let Some(input) = &mut self.search.input {
            match key.code {
                KeyCode::Esc => self.search.input = None,
                KeyCode::Enter => {
                    let query = input.to_lowercase();
                    self.search.input = None;
                    if !query.is_empty() {
                        self.search.query = query;
                        self.search.preview_match = None;
                        self.find(&panes, false);
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) if !control && !c.is_control() => input.push(c),
                _ => {}
            }
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char('/') => self.search.input = Some(String::new()),
            KeyCode::Char('n') => self.find(&panes, false),
            KeyCode::Char('N') => self.find(&panes, true),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Outline => Focus::Preview,
                    Focus::Preview => Focus::Outline,
                };
            }
            KeyCode::Char('[') => self.pane_sizes.resize(&panes, false),
            KeyCode::Char(']') => self.pane_sizes.resize(&panes, true),
            KeyCode::Char('=') => self.pane_sizes = PaneSizes::default(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('p') => {
                if self
                    .selected_action()
                    .is_some_and(|action| !action.disabled)
                {
                    self.composing = true;
                    self.status.clear();
                } else if let Some(action) = self.selected_action() {
                    self.status = action.hint.clone();
                }
            }
            _ => self.navigate(key, &panes),
        }
        false
    }

    fn find(&mut self, panes: &Panes, backward: bool) {
        let query = &self.search.query;
        if query.is_empty() {
            return;
        }

        self.status = match self.focus {
            Focus::Outline => {
                let rows = &self.snapshot.projection.nodes;
                if let Some(index) = next_match(rows.len(), self.selected, backward, |&index| {
                    rows[index].label.to_lowercase().contains(query)
                }) {
                    if index != self.selected {
                        self.selected = index;
                        self.source_scroll = 0;
                        self.search.preview_match = None;
                    }
                    "Outline match · n/N next/previous".into()
                } else {
                    "No outline matches.".into()
                }
            }
            Focus::Preview => {
                let lines = self.source_lines();
                let first = lines.start;
                let current = self.search.preview_match.map_or(
                    self.source_scroll
                        .min(panes.source_scroll_limit(lines.len())),
                    |line| line - first,
                );
                if let Some(index) = next_match(lines.len(), current, backward, |&index| {
                    self.snapshot
                        .preview
                        .line(first + index)
                        .to_lowercase()
                        .contains(query)
                }) {
                    self.source_scroll = index.min(panes.source_scroll_limit(lines.len()));
                    self.search.preview_match = Some(first + index);
                    format!("Match at line {} · n/N next/previous", first + index + 1)
                } else {
                    "No matches in this section.".into()
                }
            }
        };
    }

    fn navigate(&mut self, key: KeyEvent, panes: &Panes) {
        if let Some(jump) = OutlineJump::from_key(key) {
            if let Focus::Outline = self.focus
                && let Some(target) = jump.target(&self.snapshot.projection.nodes, self.selected)
            {
                self.selected = target;
                self.source_scroll = 0;
                self.search.preview_match = None;
            }
            return;
        }

        let selection_before = self.selected;
        let (position, last, page) = match self.focus {
            Focus::Outline => (
                &mut self.selected,
                self.snapshot.projection.nodes.len().saturating_sub(1),
                usize::from(panes.outline_height.max(1)),
            ),
            Focus::Preview => {
                let last = panes.source_scroll_limit(self.source_lines().len());
                (
                    &mut self.source_scroll,
                    last,
                    usize::from(panes.source_height.saturating_sub(1).max(1)),
                )
            }
        };
        let current = (*position).min(last);
        *position = match key.code {
            KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (current + 1).min(last),
            KeyCode::PageUp => current.saturating_sub(page),
            KeyCode::PageDown => (current + page).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => return,
        };
        if self.selected != selection_before {
            self.source_scroll = 0;
        }
        if self.selected != selection_before || matches!(self.focus, Focus::Preview) {
            self.search.preview_match = None;
        }
    }

    fn source_lines(&self) -> Range<usize> {
        self.selected_row().map_or(0..0, |row| {
            let line_count = self.snapshot.preview.line_count();
            let first = row.line.saturating_sub(1).min(line_count);
            let end = row
                .end_line
                .unwrap_or(row.line)
                .max(row.line)
                .min(line_count);
            first..end
        })
    }

    fn refresh(&mut self) {
        match Snapshot::load(&self.file) {
            Ok(snapshot) => {
                self.status = "Document refreshed.".into();
                self.replace_snapshot(snapshot);
            }
            Err(error) => self.status = format!("Refresh failed. {error:#}"),
        }
    }

    fn replace_snapshot(&mut self, snapshot: Snapshot) {
        let retained = self.selected_row().and_then(|row| {
            snapshot.projection.nodes.iter().position(|candidate| {
                if let Some(key) = &row.selection_key {
                    candidate.selection_key.as_ref() == Some(key)
                } else {
                    snapshot.projection.revision == self.snapshot.projection.revision
                        && candidate.id == row.id
                }
            })
        });
        if let Some(index) = retained {
            self.selected = index;
        } else {
            self.selected = self
                .selected
                .min(snapshot.projection.nodes.len().saturating_sub(1));
            if self.composing {
                self.composing = false;
                self.status =
                    "Selected item changed. Your draft is kept. Select a target and press p."
                        .into();
            }
        }
        self.snapshot = snapshot;
        self.source_scroll = 0;
        self.search.preview_match = None;
    }

    fn submit(&mut self) {
        if self.running.is_some() {
            self.status = "The previous dispatch is still running. Your draft is kept.".into();
            return;
        }
        if self.selected_action().is_none_or(|action| action.disabled) {
            self.status = "Choose a promptable item. Your draft is kept.".into();
            return;
        }
        let Some(row) = self.selected_row() else {
            return;
        };
        let revision = self.snapshot.projection.revision.clone();
        let id = row.id.clone();
        let target = match &row.selection_key {
            Some(key) => DraftTarget::Stable(key.clone()),
            None => DraftTarget::Snapshot {
                revision: revision.clone(),
                id: id.clone(),
            },
        };
        let attempt = DraftAttempt::for_submission(self.draft_attempt.take(), target, &self.draft);
        let receipt = attempt.receipt.clone();
        self.draft_attempt = Some(attempt);
        let submission = DocumentPromptSubmission {
            revision,
            target: id,
            draft: self.draft.clone(),
            request_id: receipt,
        };
        let (content, active_cycle) = match persist_prompt(&self.file, submission, SOURCE) {
            Ok(result) => result,
            Err(PromptFailure::AlreadyPlaced) => {
                self.finish_draft();
                self.status = "This prompt was already saved. Press r to refresh.".into();
                return;
            }
            Err(error) => {
                self.status = format!("Your draft is kept. {error} Use Ctrl+R to refresh.");
                return;
            }
        };
        self.finish_draft();
        self.status = if active_cycle {
            "Prompt saved for the active session.".into()
        } else {
            self.start_run()
        };
        match Snapshot::project(&self.file, content) {
            Ok(snapshot) => self.replace_snapshot(snapshot),
            Err(error) => {
                self.status
                    .push_str(&format!(" View refresh failed. {error:#}"));
            }
        }
    }

    fn finish_draft(&mut self) {
        self.draft.clear();
        self.draft_attempt = None;
        self.composing = false;
    }

    fn start_run(&mut self) -> String {
        // Persistence already checked the root. Resolve it again for the
        // process, which must run in the document's project.
        let Some(root) = agent_doc_project_root_io::project_root_containing(&self.file) else {
            return "Prompt saved. Dispatch could not find the document's project.".into();
        };
        match spawn_prompt_run(&root, &self.file, Stdio::null()) {
            Ok(mut child) => {
                let (sender, receiver) = mpsc::channel();
                thread::spawn(move || {
                    let result = child.wait();
                    // The Explorer may have closed while the agent was running.
                    if let Err(error) = sender.send(result)
                        && let Err(error) = error.0
                    {
                        eprintln!("[explore] could not observe the prompt run: {error}");
                    }
                });
                self.running = Some(receiver);
                "Prompt saved. Dispatch started.".into()
            }
            Err(error) => format!("Prompt saved. Dispatch failed. {error:#}"),
        }
    }

    fn observe_run(&mut self) {
        let Some(receiver) = &self.running else {
            return;
        };
        self.status = match receiver.try_recv() {
            Ok(Ok(status)) if status.success() => {
                "Dispatch finished. Press r to refresh the document.".into()
            }
            Ok(Ok(status)) => format!("Prompt saved. Dispatch exited with {status}."),
            Ok(Err(error)) => format!("Prompt saved. Could not observe dispatch. {error}"),
            Err(TryRecvError::Disconnected) => {
                "Prompt saved. The dispatch receipt is unavailable.".into()
            }
            Err(TryRecvError::Empty) => return,
        };
        self.running = None;
    }

    fn render(&self, output: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
        queue!(output, Clear(ClearType::All))?;
        if width > 0 && height > 0 {
            let title = &self.snapshot.projection.view.title;
            print_at(
                output,
                0,
                0,
                width,
                &format!("{title} · {}", self.file.display()),
            )?;
        }
        if height >= 5 && width > 0 {
            if self.composing {
                self.render_composer(output, width, height)?;
            } else {
                self.render_panes(output, self.pane_sizes.fit(width, height))?;
            }
            let search_prompt;
            let status = if let Some(input) = &self.search.input {
                let pane = match self.focus {
                    Focus::Outline => "Outline",
                    Focus::Preview => "Preview",
                };
                search_prompt = format!("{pane} /{input}");
                &search_prompt
            } else {
                &self.status
            };
            print_at(output, 0, height - 2, width, status)?;
        }
        if height >= 2 && width > 0 {
            let keys = if self.composing {
                "Ctrl+S send · Ctrl+R refresh · Esc keep draft and return"
            } else if self.search.input.is_some() {
                "Enter find · Esc cancel"
            } else {
                match self.focus {
                    Focus::Outline => {
                        "Outline · / search · Tab switch · PgUp/Dn · [ ] resize · q quit · ↑/↓ select · = reset · p prompt · r refresh"
                    }
                    Focus::Preview => {
                        "Preview · / search · Tab switch · PgUp/Dn · [ ] resize · q quit · ↑/↓ scroll · = reset · p prompt · r refresh"
                    }
                }
            };
            print_at(output, 0, height - 1, width, keys)?;
        }
        output.flush()
    }

    fn render_panes(&self, output: &mut impl Write, panes: Panes) -> io::Result<()> {
        let nodes = &self.snapshot.projection.nodes;
        let first = self
            .selected
            .saturating_sub(usize::from(panes.outline_height).saturating_sub(1));
        for (offset, row) in nodes
            .iter()
            .skip(first)
            .take(panes.outline_height.into())
            .enumerate()
        {
            row.render(
                output,
                panes.outline_top + offset as u16,
                panes.outline_width,
                first + offset == self.selected,
            )?;
        }
        let lines = self.source_lines();
        let first = lines.start
            + self
                .source_scroll
                .min(panes.source_scroll_limit(lines.len()));
        for (offset, line) in (0..panes.source_height).zip(first..lines.end) {
            self.snapshot.preview.print_line(
                output,
                panes.source_left,
                panes.source_top + offset,
                panes.source_width,
                line,
                self.search.preview_match == Some(line),
            )?;
        }
        Ok(())
    }

    fn render_composer(&self, output: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
        let view = &self.snapshot.projection.view;
        if let (Some(row), Some(action)) = (self.selected_row(), self.selected_action()) {
            print_at(
                output,
                0,
                1,
                width,
                &format!("{} · {} · {}", view.prompt_label, action.label, row.label),
            )?;
            print_at(output, 0, 2, width, &action.hint)?;
        }
        let available = height.saturating_sub(5);
        let draft = wrap(&self.draft, usize::from(width));
        let first = draft.len().saturating_sub(available.into());
        for (index, line) in draft.iter().skip(first).take(available.into()).enumerate() {
            print_at(output, 0, index as u16 + 3, width, line)?;
        }
        Ok(())
    }
}

enum OutlineJump {
    Parent,
    FirstChild,
    PreviousSibling,
    NextSibling,
}

impl OutlineJump {
    fn from_key(key: KeyEvent) -> Option<Self> {
        match (key.code, key.modifiers) {
            (KeyCode::Left, KeyModifiers::NONE) => Some(Self::Parent),
            (KeyCode::Right, KeyModifiers::NONE) => Some(Self::FirstChild),
            (KeyCode::Up, KeyModifiers::CONTROL) => Some(Self::PreviousSibling),
            (KeyCode::Down, KeyModifiers::CONTROL) => Some(Self::NextSibling),
            _ => None,
        }
    }

    fn target(self, rows: &[Row], selected: usize) -> Option<usize> {
        let depth = rows.get(selected)?.depth;
        // Rows are the outline in preorder. A shallower row ends the search
        // for siblings so a jump cannot enter a different parent's subtree.
        match self {
            Self::Parent => rows[..selected].iter().rposition(|row| row.depth < depth),
            Self::FirstChild => rows
                .get(selected + 1)
                .filter(|row| row.depth.checked_sub(1) == Some(depth))
                .map(|_| selected + 1),
            Self::PreviousSibling => rows[..selected]
                .iter()
                .enumerate()
                .rev()
                .take_while(|(_, row)| row.depth >= depth)
                .find_map(|(index, row)| (row.depth == depth).then_some(index)),
            Self::NextSibling => rows
                .iter()
                .enumerate()
                .skip(selected + 1)
                .take_while(|(_, row)| row.depth >= depth)
                .find_map(|(index, row)| (row.depth == depth).then_some(index)),
        }
    }
}

struct Panes {
    outline_top: u16,
    outline_height: u16,
    outline_width: u16,
    source_top: u16,
    source_left: u16,
    source_height: u16,
    source_width: u16,
}

impl Panes {
    fn source_scroll_limit(&self, line_count: usize) -> usize {
        line_count.saturating_sub(usize::from(self.source_height.max(1)))
    }
}

#[derive(Default)]
struct PaneSizes {
    outline_columns: Option<u16>,
    outline_rows: Option<u16>,
}

impl PaneSizes {
    const MIN_COLUMNS: u16 = 16;
    const MIN_ROWS: u16 = 1;

    fn fit(&self, width: u16, height: u16) -> Panes {
        let rows = height.saturating_sub(3);
        if width >= 80 {
            let columns = width - 1;
            let outline = self.outline_columns.unwrap_or((width / 2).min(52));
            let outline = Self::clamp(outline, columns, Self::MIN_COLUMNS);
            Panes {
                outline_top: 1,
                outline_height: rows,
                outline_width: outline,
                source_top: 1,
                source_left: outline + 1,
                source_height: rows,
                source_width: columns - outline,
            }
        } else {
            let outline = self.outline_rows.unwrap_or(rows.div_ceil(2));
            let outline = Self::clamp(outline, rows, Self::MIN_ROWS);
            Panes {
                outline_top: 1,
                outline_height: outline,
                outline_width: width,
                source_top: outline + 1,
                source_left: 0,
                source_height: rows - outline,
                source_width: width,
            }
        }
    }

    fn resize(&mut self, panes: &Panes, grow_outline: bool) {
        let (preference, current, available, step, minimum) = if panes.source_left > 0 {
            (
                &mut self.outline_columns,
                panes.outline_width,
                panes.outline_width + panes.source_width,
                4,
                Self::MIN_COLUMNS,
            )
        } else {
            (
                &mut self.outline_rows,
                panes.outline_height,
                panes.outline_height + panes.source_height,
                1,
                Self::MIN_ROWS,
            )
        };
        let requested = if grow_outline {
            current.saturating_add(step)
        } else {
            current.saturating_sub(step)
        };
        let size = Self::clamp(requested, available, minimum);
        if size != current {
            *preference = Some(size);
        }
    }

    fn clamp(outline: u16, available: u16, minimum: u16) -> u16 {
        let minimum = minimum.min(available / 2);
        outline.clamp(minimum, available - minimum)
    }
}

fn printable(text: &str) -> impl Iterator<Item = char> + '_ {
    // Terminal markup always comes from Crossterm, even when document text,
    // view labels, paths, or error messages contain their own controls.
    text.chars().map(terminal_char)
}

fn terminal_char(c: char) -> char {
    if c.is_control() { ' ' } else { c }
}

fn clip_to_width<'a>(text: &'a str, space: &mut usize) -> &'a str {
    for (offset, c) in text.char_indices() {
        let cells = terminal_char(c).width().unwrap_or_default();
        if cells > *space {
            return &text[..offset];
        }
        *space -= cells;
    }
    text
}

fn print_at(output: &mut impl Write, x: u16, y: u16, width: u16, text: &str) -> io::Result<()> {
    let mut columns = usize::from(width);
    let text: String = printable(clip_to_width(text, &mut columns)).collect();
    queue!(output, MoveTo(x, y), Print(text))
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for source in text.split('\n') {
        let mut line = String::new();
        let mut cells = 0;
        for c in printable(source) {
            let next = c.width().unwrap_or_default();
            if next > width {
                continue;
            }
            if cells + next > width {
                lines.push(std::mem::take(&mut line));
                cells = 0;
            }
            line.push(c);
            cells += next;
        }
        lines.push(line);
    }
    lines
}

struct Terminal {
    output: io::Stdout,
}

impl Terminal {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("enable terminal raw mode")?;
        let mut session = Self {
            output: io::stdout(),
        };
        execute!(
            session.output,
            EnterAlternateScreen,
            DisableLineWrap,
            EnableBracketedPaste,
            Hide
        )?;
        Ok(session)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Try both operations even if the emulator has stopped accepting output.
        let screen = execute!(
            self.output,
            SetAttribute(Attribute::Reset),
            Show,
            DisableBracketedPaste,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        if let Err(error) = terminal::disable_raw_mode() {
            eprintln!("[explore] could not restore terminal input: {error}");
        }
        if let Err(error) = screen {
            eprintln!("[explore] could not restore terminal screen: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_is_reused_only_for_an_exact_draft_and_target() {
        let target = || DraftTarget::Stable("document".into());
        let initial = DraftAttempt::for_submission(None, target(), "Question");
        let receipt = initial.receipt.clone();
        let retry = DraftAttempt::for_submission(Some(initial), target(), "Question");
        assert_eq!(retry.receipt, receipt);
        let edited = DraftAttempt::for_submission(Some(retry), target(), "Question revised");
        assert_ne!(edited.receipt, receipt);
        let edited_receipt = edited.receipt.clone();
        let selected_other_row = DraftAttempt::for_submission(
            Some(edited),
            DraftTarget::Stable("another row".into()),
            "Question revised",
        );
        assert_ne!(selected_other_row.receipt, edited_receipt);
    }

    #[test]
    fn outline_selection_covers_highlights_and_task_badges_without_leaking_style() {
        let label = "**é界** plain";
        let preview = SourcePreview::new(label.to_owned(), true);
        let mut row = Row {
            id: String::new(),
            selection_key: None,
            label: label.to_owned(),
            line: 1,
            end_line: None,
            depth: 0,
            action: String::new(),
            task: Some(Task {
                label: "Pending".into(),
            }),
            highlights: preview.outline_highlights(label, 0),
        };
        let mut output = Vec::new();
        row.render(&mut output, 0, 28, true).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .ends_with("› \x1b[1m**é界**\x1b[0m\x1b[7m plain [Pending]\x1b[0m")
        );

        let mut output = Vec::new();
        row.render(&mut output, 0, 6, false).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .ends_with("  \x1b[1m**é\x1b[0m")
        );

        row.highlights.clear();
        let mut output = Vec::new();
        row.render(&mut output, 0, 28, true).unwrap();
        assert!(
            String::from_utf8(output)
                .unwrap()
                .ends_with("› **é界** plain [Pending]\x1b[0m")
        );
    }

    #[test]
    fn navigation_pages_the_focused_pane_and_keeps_preview_position_on_focus_change() {
        let nodes = [0, 1, 2, 1, 2, 2, 0, 1]
            .into_iter()
            .enumerate()
            .map(|(index, depth)| Row {
                id: index.to_string(),
                selection_key: None,
                label: format!("Heading {index}"),
                line: index + 1,
                end_line: Some(index + 12),
                depth,
                action: String::new(),
                task: None,
                highlights: Vec::new(),
            })
            .collect();
        let snapshot = Snapshot {
            preview: SourcePreview::new("source\n".repeat(20), false),
            projection: Projection {
                revision: String::new(),
                nodes,
                view: View {
                    title: String::new(),
                    prompt_label: String::new(),
                    prompt_actions: HashMap::new(),
                },
            },
        };
        let mut explorer = Explorer::new(PathBuf::new(), snapshot);
        let mut press = |code, modifiers| {
            assert!(!explorer.key(KeyEvent::new(code, modifiers), 66, 11));
            (explorer.selected, explorer.source_scroll)
        };
        let plain = KeyModifiers::NONE;
        assert_eq!(press(KeyCode::PageDown, plain), (4, 0));
        assert_eq!(press(KeyCode::Tab, plain), (4, 0));
        assert_eq!(press(KeyCode::PageDown, plain), (4, 3));
        assert_eq!(press(KeyCode::Down, plain), (4, 4));
        assert_eq!(press(KeyCode::BackTab, KeyModifiers::SHIFT), (4, 4));
        assert_eq!(press(KeyCode::Tab, plain), (4, 4));
        assert_eq!(press(KeyCode::End, plain), (4, 8));
        assert_eq!(press(KeyCode::PageUp, plain), (4, 5));
        assert_eq!(press(KeyCode::BackTab, KeyModifiers::SHIFT), (4, 5));
        assert_eq!(press(KeyCode::PageDown, plain), (7, 0));
        assert_eq!(press(KeyCode::PageUp, plain), (3, 0));

        let control = KeyModifiers::CONTROL;
        assert_eq!(press(KeyCode::Down, control), (3, 0));
        assert_eq!(press(KeyCode::Up, control), (1, 0));
        assert_eq!(press(KeyCode::Right, plain), (2, 0));
        assert_eq!(press(KeyCode::Right, plain), (2, 0));
        assert_eq!(press(KeyCode::Down, control), (2, 0));
        assert_eq!(press(KeyCode::Left, plain), (1, 0));
        assert_eq!(press(KeyCode::Down, control), (3, 0));
        assert_eq!(press(KeyCode::Right, plain), (4, 0));
        assert_eq!(press(KeyCode::Down, control), (5, 0));
        assert_eq!(press(KeyCode::Up, control), (4, 0));
        assert_eq!(press(KeyCode::Tab, plain), (4, 0));
        assert_eq!(press(KeyCode::Down, plain), (4, 1));
        assert_eq!(press(KeyCode::Up, control), (4, 1));
        assert_eq!(press(KeyCode::Left, plain), (4, 1));
        assert_eq!(press(KeyCode::Tab, plain), (4, 1));
        assert_eq!(press(KeyCode::Left, plain), (3, 0));
        assert_eq!(press(KeyCode::Left, plain), (0, 0));
        assert_eq!(press(KeyCode::Left, plain), (0, 0));
        assert_eq!(press(KeyCode::Down, control), (6, 0));
        assert_eq!(press(KeyCode::Right, plain), (7, 0));
        assert_eq!(press(KeyCode::Up, control), (7, 0));
    }

    #[test]
    fn slash_search_respects_focus_and_repeats_past_the_last_scroll_position() {
        let nodes = ["Document", "Match one", "Other", "Match two"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| Row {
                id: index.to_string(),
                selection_key: None,
                label: label.into(),
                line: 1,
                end_line: Some(5),
                depth: 0,
                action: String::new(),
                task: None,
                highlights: Vec::new(),
            })
            .collect();
        let snapshot = Snapshot {
            preview: SourcePreview::new(
                "first\nother\nthird\nMATCH one\nmatch two\n".into(),
                false,
            ),
            projection: Projection {
                revision: String::new(),
                nodes,
                view: View {
                    title: String::new(),
                    prompt_label: String::new(),
                    prompt_actions: HashMap::new(),
                },
            },
        };
        let mut explorer = Explorer::new(PathBuf::new(), snapshot);
        let press = |explorer: &mut Explorer, code| {
            assert!(!explorer.key(KeyEvent::new(code, KeyModifiers::NONE), 66, 11));
        };
        for c in "/MaTcH".chars() {
            press(&mut explorer, KeyCode::Char(c));
        }
        press(&mut explorer, KeyCode::Enter);
        assert_eq!(explorer.selected, 1);
        press(&mut explorer, KeyCode::Char('n'));
        assert_eq!(explorer.selected, 3);
        press(&mut explorer, KeyCode::Char('n'));
        assert_eq!(explorer.selected, 1);
        press(&mut explorer, KeyCode::Char('N'));
        assert_eq!(explorer.selected, 3);

        // Escape leaves the current selection and repeat query available.
        press(&mut explorer, KeyCode::Char('/'));
        press(&mut explorer, KeyCode::Char('x'));
        press(&mut explorer, KeyCode::Esc);
        assert_eq!(explorer.selected, 3);
        assert_eq!(explorer.search.query, "match");

        press(&mut explorer, KeyCode::Tab);
        press(&mut explorer, KeyCode::Char('n'));
        assert_eq!(explorer.selected, 3);
        assert_eq!(explorer.source_scroll, 1);
        assert_eq!(explorer.search.preview_match, Some(3));
        press(&mut explorer, KeyCode::Char('n'));
        assert_eq!(explorer.source_scroll, 1);
        assert_eq!(explorer.search.preview_match, Some(4));
        press(&mut explorer, KeyCode::Char('n'));
        assert_eq!(explorer.search.preview_match, Some(3));
        press(&mut explorer, KeyCode::Char('N'));
        assert_eq!(explorer.search.preview_match, Some(4));

        press(&mut explorer, KeyCode::Up);
        assert_eq!(explorer.search.preview_match, None);
    }

    #[test]
    fn pane_sizes_remember_each_layout_and_fit_smaller_terminals() {
        let mut sizes = PaneSizes::default();
        let wide = sizes.fit(110, 30);
        let narrow = sizes.fit(66, 30);
        assert_eq!((wide.outline_width, wide.source_width), (52, 57));
        assert_eq!((narrow.outline_height, narrow.source_height), (14, 13));

        sizes.resize(&wide, true);
        sizes.resize(&narrow, false);
        let wide = sizes.fit(110, 30);
        let narrow = sizes.fit(66, 30);
        assert_eq!(
            (wide.outline_width, wide.source_left, wide.source_width),
            (56, 57, 53)
        );
        assert_eq!(
            (
                narrow.outline_height,
                narrow.source_top,
                narrow.source_height
            ),
            (13, 14, 14)
        );

        let smaller = sizes.fit(80, 5);
        assert_eq!((smaller.outline_width, smaller.source_width), (56, 23));
        let short = sizes.fit(66, 5);
        assert_eq!((short.outline_height, short.source_height), (1, 1));
        assert_eq!(sizes.fit(66, 30).outline_height, 13);

        for _ in 0..100 {
            let wide = sizes.fit(110, 30);
            sizes.resize(&wide, false);
            let narrow = sizes.fit(66, 30);
            sizes.resize(&narrow, true);
        }
        let wide = sizes.fit(110, 30);
        let narrow = sizes.fit(66, 30);
        assert_eq!((wide.outline_width, wide.source_width), (16, 93));
        assert_eq!((narrow.outline_height, narrow.source_height), (26, 1));
    }

    #[test]
    fn terminal_text_neutralizes_controls_and_clips_wide_characters() {
        let mut output = Vec::new();
        print_at(&mut output, 0, 0, 6, "a\x1b\u{9f}界bc").unwrap();
        assert!(output.ends_with("a  界b".as_bytes()));
        assert_eq!(wrap("ab界\nc", 3), ["ab", "界", "c"]);
    }
}
