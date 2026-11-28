//! Terminal host for the shared Lazily View-backed Document Explorer.

mod editor;
mod highlight;

use crate::document_explorer::{
    PromptFailure, explorer_active_prompts, load_open_cycle, persist_prompt, spawn_prompt_run,
};
use agent_doc_editor_surface_io::{DocumentPromptSubmission, project_document_prompts_with_active};
use agent_doc_markdown_ast::overlay::parse_open_marker_line;
use agent_doc_watch_io::{ResourceAwareWatcher, WatchFallback};
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
use notify::{Event as FileEvent, RecursiveMode};
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
    let updates = match DocumentUpdates::watch(&file) {
        Ok(updates) => Some(updates),
        Err(error) => {
            eprintln!("[explore] automatic refresh unavailable: {error:#}");
            None
        }
    };
    let mut explorer = Explorer::open(file)?;
    let mut screen = Terminal::enter()?;
    explorer.interact(&mut screen, updates)
}

struct DocumentUpdates {
    watcher: ResourceAwareWatcher,
    events: Receiver<notify::Result<FileEvent>>,
}

impl DocumentUpdates {
    fn watch(file: &Path) -> Result<Self> {
        let directory = file.parent().context("find document directory")?;
        let (sender, events) = mpsc::channel();
        let mut watcher = ResourceAwareWatcher::new(sender, Duration::from_millis(500))?;
        // Watch the directory so atomic saves do not leave the old inode watched.
        watcher.watch(directory, RecursiveMode::NonRecursive)?;
        Ok(Self { watcher, events })
    }

    fn take_change(&mut self, file: &Path) -> bool {
        let mut changed = false;
        for event in self.events.try_iter() {
            match event {
                Ok(event) => {
                    changed |= event.need_rescan()
                        || (!event.kind.is_access() && event.paths.iter().any(|path| path == file));
                }
                Err(error) => match self.watcher.recover_from_event_error(&error) {
                    Ok(WatchFallback::Activated) => changed = true,
                    Ok(WatchFallback::Unchanged) => eprintln!("[explore] file watch: {error}"),
                    Err(fallback) => {
                        eprintln!("[explore] file watch: {error}; fallback: {fallback}");
                    }
                },
            }
        }
        changed
    }
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
    request_id: String,
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

    fn selection_target(&self, index: usize) -> Option<SelectionTarget> {
        self.projection
            .nodes
            .get(index)
            .map(|row| SelectionTarget::new(row, &self.projection.revision))
    }

    fn resolve_selection(&self, target: &SelectionTarget) -> Option<usize> {
        let nodes = &self.projection.nodes;
        match target {
            SelectionTarget::Stable(key) => nodes
                .iter()
                .position(|row| row.selection_key.as_ref() == Some(key)),
            SelectionTarget::Snapshot { revision, id } if self.projection.revision == *revision => {
                nodes.iter().position(|row| row.id == *id)
            }
            SelectionTarget::Snapshot { .. } => None,
        }
    }
}

#[derive(Clone, PartialEq)]
enum SelectionTarget {
    Stable(String),
    Snapshot { revision: String, id: String },
}

impl SelectionTarget {
    fn new(row: &Row, revision: &str) -> Self {
        match &row.selection_key {
            Some(key) => Self::Stable(key.clone()),
            None => Self::Snapshot {
                revision: revision.into(),
                id: row.id.clone(),
            },
        }
    }
}

#[derive(Default)]
struct SelectionHistory {
    entries: Vec<SelectionTarget>,
    position: usize,
}

impl SelectionHistory {
    fn visit(&mut self, target: SelectionTarget) {
        self.entries.truncate(self.position + 1);
        self.entries.push(target);
        self.position = self.entries.len() - 1;
    }

    fn step(&mut self, snapshot: &Snapshot, forward: bool) -> Option<usize> {
        let position = if forward {
            self.position + 1
        } else {
            self.position.checked_sub(1)?
        };
        let selected = snapshot.resolve_selection(self.entries.get(position)?)?;
        self.position = position;
        Some(selected)
    }

    fn refresh(&mut self, snapshot: &Snapshot, current: Option<SelectionTarget>) {
        let Some(current) = current else {
            *self = Self::default();
            return;
        };
        let previous_position = self.position;
        self.position = 0;
        for (index, target) in std::mem::take(&mut self.entries).into_iter().enumerate() {
            let target = if index == previous_position {
                current.clone()
            } else if snapshot.resolve_selection(&target).is_some() {
                target
            } else {
                continue;
            };
            if self.entries.last() != Some(&target) {
                self.entries.push(target);
            }
            if index == previous_position {
                self.position = self.entries.len() - 1;
            }
        }
        if self.entries.is_empty() {
            self.visit(current);
        }
    }
}

struct DraftAttempt {
    receipt: String,
    target: SelectionTarget,
    text: String,
}

impl DraftAttempt {
    fn for_submission(previous: Option<Self>, target: SelectionTarget, text: &str) -> Self {
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
    history: SelectionHistory,
    source_scroll: usize,
    focus: Focus,
    search: Search,
    pane_sizes: PaneSizes,
    status: String,
    review_top: Option<usize>,
    draft: String,
    draft_attempt: Option<DraftAttempt>,
    running: Option<Receiver<Result<std::process::ExitStatus, io::Error>>>,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    Continue,
    EditDraft,
    Quit,
}

impl Explorer {
    fn open(file: PathBuf) -> Result<Self> {
        let snapshot = Snapshot::load(&file)?;
        Ok(Self::new(file, snapshot))
    }

    fn new(file: PathBuf, snapshot: Snapshot) -> Self {
        let mut history = SelectionHistory::default();
        if let Some(target) = snapshot.selection_target(0) {
            history.visit(target);
        }
        Self {
            file,
            snapshot,
            selected: 0,
            history,
            source_scroll: 0,
            focus: Focus::default(),
            search: Search::default(),
            pane_sizes: PaneSizes::default(),
            status: String::new(),
            review_top: None,
            draft: String::new(),
            draft_attempt: None,
            running: None,
        }
    }

    fn selected_row(&self) -> Option<&Row> {
        self.snapshot.projection.nodes.get(self.selected)
    }

    fn visit_selection(&mut self, index: usize) {
        if index != self.selected
            && let Some(target) = self.snapshot.selection_target(index)
        {
            self.history.visit(target);
            self.show_selection(index);
        }
    }

    fn show_selection(&mut self, index: usize) {
        self.selected = index;
        self.source_scroll = 0;
        self.search.preview_match = None;
    }

    fn selected_action(&self) -> Option<&PromptAction> {
        let row = self.selected_row()?;
        self.snapshot
            .projection
            .view
            .prompt_actions
            .get(&row.action)
    }

    fn selected_prompt_context(&self) -> &str {
        let Some(row) = self.selected_row() else {
            return "";
        };
        if row.id == "document" {
            return &row.label;
        }
        self.source_lines()
            .map(|line| self.snapshot.preview.line(line))
            .find(|line| {
                let text = line.trim_start();
                !text.is_empty()
                    && (!text.starts_with("<!--") || parse_open_marker_line(text).is_some())
            })
            .unwrap_or(&row.label)
    }

    fn answer_row(&self) -> Option<usize> {
        let receipt = &self.selected_row()?.task.as_ref()?.request_id;
        let rows = &self.snapshot.projection.nodes;
        let index = OutlineJump::FirstChild.target(rows, self.selected)?;
        (rows[index].id.strip_prefix("prompt-answer:") == Some(receipt)).then_some(index)
    }

    fn interact(
        &mut self,
        screen: &mut Terminal,
        mut updates: Option<DocumentUpdates>,
    ) -> Result<()> {
        let mut redraw = true;
        let mut refresh_pending = false;
        loop {
            let dispatch_finished = self.observe_run();
            refresh_pending |= dispatch_finished;
            if let Some(updates) = &mut updates {
                refresh_pending |= updates.take_change(&self.file);
            }
            // Review owns the captured revision until Ctrl+R or exit. A
            // background document update must not silently rebase a draft.
            if refresh_pending && self.review_top.is_none() {
                redraw |= self.refresh_changed();
                refresh_pending = false;
            }
            let (width, height) = terminal::size()?;
            if redraw || dispatch_finished {
                self.render(&mut screen.output, width, height)?;
                redraw = false;
            }
            // Multiplex terminal input with the file-watcher and dispatch
            // channels. Only their events trigger an authoritative text read.
            if (updates.is_some() || self.running.is_some())
                && !event::poll(Duration::from_millis(125))?
            {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    match self.key(key, width, height) {
                        KeyAction::Continue => {}
                        KeyAction::EditDraft => self.edit_draft(screen)?,
                        KeyAction::Quit => return Ok(()),
                    }
                }
                Event::Paste(text) => {
                    if let Some(input) = &mut self.search.input {
                        input.extend(text.chars().filter(|c| !c.is_control()));
                    }
                }
                _ => {}
            }
            redraw = true;
        }
    }

    fn key(&mut self, key: KeyEvent, width: u16, height: u16) -> KeyAction {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            return KeyAction::Quit;
        }
        if let Some(top) = self.review_top {
            match key.code {
                KeyCode::Esc => {
                    self.review_top = None;
                    self.status = if self.draft.trim().is_empty() {
                        String::new()
                    } else {
                        "Draft kept. Press Enter on a promptable item to edit it.".into()
                    };
                }
                KeyCode::Char('s') if control => self.submit(),
                KeyCode::Char('r') if control => self.refresh(),
                KeyCode::Enter | KeyCode::Char('p') if key.modifiers.is_empty() => {
                    return KeyAction::EditDraft;
                }
                KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End => {
                    let page = usize::from(height.saturating_sub(5)).max(1);
                    let last = wrap(&self.draft, usize::from(width))
                        .len()
                        .saturating_sub(page);
                    self.review_top = Some(match key.code {
                        KeyCode::PageUp => top.min(last).saturating_sub(page),
                        KeyCode::PageDown => top.saturating_add(page).min(last),
                        KeyCode::Home => 0,
                        _ => last,
                    });
                }
                _ => {}
            }
            return KeyAction::Continue;
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
            return KeyAction::Continue;
        }

        if key.modifiers == KeyModifiers::ALT && matches!(key.code, KeyCode::Left | KeyCode::Right)
        {
            if let Some(index) = self
                .history
                .step(&self.snapshot, key.code == KeyCode::Right)
            {
                self.show_selection(index);
            }
            return KeyAction::Continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return KeyAction::Quit,
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
            KeyCode::Enter | KeyCode::Char('p') if key.modifiers.is_empty() => {
                if key.code == KeyCode::Enter
                    && let Some(answer) = self.answer_row()
                {
                    self.visit_selection(answer);
                } else if self
                    .selected_action()
                    .is_some_and(|action| !action.disabled)
                {
                    self.review_top = Some(0);
                    return KeyAction::EditDraft;
                } else if let Some(action) = self.selected_action() {
                    self.status = action.hint.clone();
                }
            }
            _ => self.navigate(key, &panes),
        }
        KeyAction::Continue
    }

    fn edit_draft(&mut self, screen: &mut Terminal) -> Result<()> {
        let outcome =
            match editor::DraftEditor::prepare(&self.draft, self.selected_prompt_context()) {
                Ok(mut editor) => {
                    screen.suspend()?;
                    let outcome = editor.edit();
                    screen.resume()?;
                    outcome
                }
                Err(error) => Err(error),
            };
        self.status = match outcome {
            Ok(draft) => {
                self.draft = draft;
                self.review_top = Some(0);
                if self.draft.trim().is_empty() {
                    "No draft to send. Press Enter to reopen the editor.".into()
                } else {
                    "Review your draft before sending.".into()
                }
            }
            Err(error) => format!("Editor did not save a draft. Previous draft kept. {error:#}"),
        };
        Ok(())
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
                    self.visit_selection(index);
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
                self.visit_selection(target);
            }
            return;
        }

        let (position, last, page) = match self.focus {
            Focus::Outline => (
                self.selected,
                self.snapshot.projection.nodes.len().saturating_sub(1),
                usize::from(panes.outline_height.max(1)),
            ),
            Focus::Preview => {
                let last = panes.source_scroll_limit(self.source_lines().len());
                (
                    self.source_scroll,
                    last,
                    usize::from(panes.source_height.saturating_sub(1).max(1)),
                )
            }
        };
        let current = position.min(last);
        let next = match key.code {
            KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (current + 1).min(last),
            KeyCode::PageUp => current.saturating_sub(page),
            KeyCode::PageDown => (current + page).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => return,
        };
        match self.focus {
            Focus::Outline => self.visit_selection(next),
            Focus::Preview => {
                self.source_scroll = next;
                self.search.preview_match = None;
            }
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

    fn refresh_changed(&mut self) -> bool {
        let result = (|| {
            let content =
                agent_doc_document_realtime_io::peek_current_document_content(&self.file, SOURCE)?;
            if agent_doc_hash::content_hash(&content) == self.snapshot.projection.revision {
                return Ok(None);
            }
            Snapshot::project(&self.file, content).map(Some)
        })();
        match result {
            Ok(Some(snapshot)) => self.replace_snapshot(snapshot),
            Ok(None) => return false,
            Err(error) => self.status = format!("Automatic refresh failed. {error:#}"),
        }
        true
    }

    fn replace_snapshot(&mut self, snapshot: Snapshot) {
        let retained = self
            .snapshot
            .selection_target(self.selected)
            .and_then(|target| snapshot.resolve_selection(&target));
        if let Some(index) = retained {
            self.selected = index;
        } else {
            self.selected = self
                .selected
                .min(snapshot.projection.nodes.len().saturating_sub(1));
            self.source_scroll = 0;
            if self.review_top.take().is_some() {
                self.status =
                    "Selected item changed. Your draft is kept. Select a target and press Enter."
                        .into();
            }
        }
        self.history
            .refresh(&snapshot, snapshot.selection_target(self.selected));
        self.snapshot = snapshot;
        self.search.preview_match = None;
    }

    fn submit(&mut self) {
        if self.draft.trim().is_empty() {
            self.status = "No draft to send. Press Enter to reopen the editor.".into();
            return;
        }
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
        let target = SelectionTarget::new(row, &revision);
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
        self.review_top = None;
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

    fn observe_run(&mut self) -> bool {
        let Some(receiver) = &self.running else {
            return false;
        };
        self.status = match receiver.try_recv() {
            Ok(Ok(status)) if status.success() => "Dispatch finished.".into(),
            Ok(Ok(status)) => format!("Prompt saved. Dispatch exited with {status}."),
            Ok(Err(error)) => format!("Prompt saved. Could not observe dispatch. {error}"),
            Err(TryRecvError::Disconnected) => {
                "Prompt saved. The dispatch receipt is unavailable.".into()
            }
            Err(TryRecvError::Empty) => return false,
        };
        self.running = None;
        true
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
            if let Some(top) = self.review_top {
                self.render_review(output, width, height, top)?;
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
            let keys = if self.review_top.is_some() {
                "Ctrl+S send · Enter edit · PgUp/Dn · Ctrl+R refresh · Esc back".into()
            } else if self.search.input.is_some() {
                "Enter find · Esc cancel".into()
            } else {
                let pane = match self.focus {
                    Focus::Outline => "Outline",
                    Focus::Preview => "Preview",
                };
                let enter = if self.answer_row().is_some() {
                    "Enter answer"
                } else {
                    "Enter prompt"
                };
                format!(
                    "{pane} · {enter} · / find · Tab switch · [ ] resize · q quit · Alt+←/→ history · ↑/↓ · PgUp/Dn · = reset · r refresh"
                )
            };
            print_at(output, 0, height - 1, width, &keys)?;
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

    fn render_review(
        &self,
        output: &mut impl Write,
        width: u16,
        height: u16,
        top: usize,
    ) -> io::Result<()> {
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
        let first = top.min(draft.len().saturating_sub(available.into()));
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
        let mut session = Self {
            output: io::stdout(),
        };
        session.resume()?;
        Ok(session)
    }

    fn resume(&mut self) -> Result<()> {
        terminal::enable_raw_mode().context("enable terminal raw mode")?;
        execute!(
            self.output,
            EnterAlternateScreen,
            DisableLineWrap,
            EnableBracketedPaste,
            Hide
        )
        .context("enter Explorer screen")
    }

    fn suspend(&mut self) -> Result<()> {
        // Try both operations even if the emulator has stopped accepting output.
        let screen = execute!(
            self.output,
            SetAttribute(Attribute::Reset),
            Show,
            DisableBracketedPaste,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        terminal::disable_raw_mode().context("restore terminal input")?;
        screen.context("restore terminal screen")
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Err(error) = self.suspend() {
            eprintln!("[explore] could not restore terminal: {error:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_opens_the_editor_with_the_selected_source_row() {
        let row = "- ### 📥 Prompt on **exact Markdown** [#sample]";
        let source = format!(
            "# Sample\n\n## Run <!-- agent:run -->\n{row}\n<!-- /agent:run -->\n\n## Exchange <!-- agent:exchange -->\n\n<!-- /agent:exchange -->\n"
        );
        let projection =
            serde_json::from_value(project_document_prompts_with_active(&source, &[]).unwrap())
                .unwrap();
        let snapshot = Snapshot {
            preview: SourcePreview::new(source, false),
            projection,
        };
        let mut explorer = Explorer::new(PathBuf::new(), snapshot);
        assert_eq!(explorer.selected_prompt_context(), "Document");

        let press = |explorer: &mut Explorer, code| {
            explorer.key(KeyEvent::new(code, KeyModifiers::NONE), 80, 30)
        };
        for character in "/exact".chars() {
            assert_eq!(
                press(&mut explorer, KeyCode::Char(character)),
                KeyAction::Continue
            );
        }
        assert_eq!(press(&mut explorer, KeyCode::Enter), KeyAction::Continue);
        assert_eq!(explorer.selected_prompt_context(), row);
        assert_eq!(press(&mut explorer, KeyCode::Enter), KeyAction::EditDraft);
        assert_eq!(press(&mut explorer, KeyCode::Enter), KeyAction::EditDraft);
        assert_eq!(press(&mut explorer, KeyCode::Esc), KeyAction::Continue);
        assert_eq!(press(&mut explorer, KeyCode::Tab), KeyAction::Continue);
        assert_eq!(press(&mut explorer, KeyCode::Enter), KeyAction::EditDraft);
    }

    #[test]
    fn receipt_is_reused_only_for_an_exact_draft_and_target() {
        let target = || SelectionTarget::Stable("document".into());
        let initial = DraftAttempt::for_submission(None, target(), "Question");
        let receipt = initial.receipt.clone();
        let retry = DraftAttempt::for_submission(Some(initial), target(), "Question");
        assert_eq!(retry.receipt, receipt);
        let edited = DraftAttempt::for_submission(Some(retry), target(), "Question revised");
        assert_ne!(edited.receipt, receipt);
        let edited_receipt = edited.receipt.clone();
        let selected_other_row = DraftAttempt::for_submission(
            Some(edited),
            SelectionTarget::Stable("another row".into()),
            "Question revised",
        );
        assert_ne!(selected_other_row.receipt, edited_receipt);
    }

    #[test]
    fn enter_on_the_answered_prompt_opens_its_saved_response() {
        let receipt = "0123456789abcdef0123456789abcdef";
        let source = format!(
            "# Sample\n\n## Exchange <!-- agent:exchange -->\n\n\
             ❯ Document Explorer prompt. Please answer the quoted request and include a standalone HTML comment whose text is repo-context-answer:{receipt} in your matching response.\n\
             > Where are the instructions?\n\n\
             ### Re: Instructions\n\n<!-- repo-context-answer:{receipt} -->\n\n\
             The instructions are in the ticket.\n<!-- /agent:exchange -->\n"
        );
        let projection =
            serde_json::from_value(project_document_prompts_with_active(&source, &[]).unwrap())
                .unwrap();
        let snapshot = Snapshot {
            preview: SourcePreview::new(source, false),
            projection,
        };
        let mut explorer = Explorer::new(PathBuf::new(), snapshot);
        explorer.visit_selection(
            explorer
                .snapshot
                .projection
                .nodes
                .iter()
                .position(|row| row.task.is_some())
                .expect("prompt row"),
        );
        assert!(
            explorer
                .source_lines()
                .any(|line| explorer.snapshot.preview.line(line) == "> Where are the instructions?")
        );
        explorer.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 90, 30);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(explorer.key(enter, 90, 30), KeyAction::Continue);
        assert_eq!(explorer.selected_row().unwrap().label, "Answer");
        assert!(matches!(explorer.focus, Focus::Preview));
        assert!(explorer.source_lines().any(|line| {
            explorer.snapshot.preview.line(line) == "The instructions are in the ticket."
        }));
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
                request_id: String::new(),
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
            assert_eq!(
                explorer.key(KeyEvent::new(code, modifiers), 66, 11),
                KeyAction::Continue
            );
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
            assert_eq!(
                explorer.key(KeyEvent::new(code, KeyModifiers::NONE), 66, 11),
                KeyAction::Continue
            );
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
    fn selection_history_tracks_jumps_and_search_without_reusing_stale_ids() {
        let snapshot = |revision: &str, labels: &[&str]| Snapshot {
            preview: SourcePreview::new("source\n".repeat(20), false),
            projection: Projection {
                revision: revision.into(),
                nodes: labels
                    .iter()
                    .enumerate()
                    .map(|(index, label)| Row {
                        id: index.to_string(),
                        selection_key: (*label != "unstable").then(|| (*label).into()),
                        label: (*label).into(),
                        line: 1,
                        end_line: Some(20),
                        depth: usize::from(index != 0),
                        action: String::new(),
                        task: None,
                        highlights: Vec::new(),
                    })
                    .collect(),
                view: View {
                    title: String::new(),
                    prompt_label: String::new(),
                    prompt_actions: HashMap::new(),
                },
            },
        };
        let mut explorer = Explorer::new(
            PathBuf::new(),
            snapshot("first", &["Document", "Alpha", "unstable", "Gamma"]),
        );
        let press = |explorer: &mut Explorer, code, modifiers| {
            assert_eq!(
                explorer.key(KeyEvent::new(code, modifiers), 66, 11),
                KeyAction::Continue
            );
        };
        let plain = KeyModifiers::NONE;
        let alt = KeyModifiers::ALT;
        let back = |explorer: &mut Explorer| press(explorer, KeyCode::Left, alt);
        let forward = |explorer: &mut Explorer| press(explorer, KeyCode::Right, alt);

        back(&mut explorer);
        assert_eq!(explorer.selected, 0);
        press(&mut explorer, KeyCode::Right, plain);
        assert_eq!(explorer.selected, 1);
        press(&mut explorer, KeyCode::Down, KeyModifiers::CONTROL);
        assert_eq!(explorer.selected, 2);
        for character in "/gamma".chars() {
            press(&mut explorer, KeyCode::Char(character), plain);
        }
        press(&mut explorer, KeyCode::Enter, plain);
        assert_eq!(explorer.selected, 3);
        back(&mut explorer);
        assert_eq!(explorer.selected, 2);
        back(&mut explorer);
        assert_eq!(explorer.selected, 1);

        explorer.replace_snapshot(snapshot(
            "updated",
            &["Document", "Inserted", "Alpha", "Replacement", "Gamma"],
        ));
        assert_eq!(explorer.selected, 2);
        press(&mut explorer, KeyCode::Tab, plain);
        press(&mut explorer, KeyCode::Down, plain);
        assert_eq!(explorer.source_scroll, 1);
        forward(&mut explorer);
        assert_eq!(explorer.selected, 4);
        assert_eq!(explorer.source_scroll, 0);
        back(&mut explorer);
        assert_eq!(explorer.selected, 2);
        assert!(matches!(explorer.focus, Focus::Preview));

        press(&mut explorer, KeyCode::Tab, plain);
        press(&mut explorer, KeyCode::Down, plain);
        forward(&mut explorer);
        assert_eq!(explorer.selected, 3);
        back(&mut explorer);
        assert_eq!(explorer.selected, 2);
        forward(&mut explorer);
        assert_eq!(explorer.selected, 3);

        press(&mut explorer, KeyCode::Char('/'), plain);
        back(&mut explorer);
        press(&mut explorer, KeyCode::Esc, plain);
        explorer.review_top = Some(0);
        back(&mut explorer);
        assert_eq!(explorer.selected, 3);
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
