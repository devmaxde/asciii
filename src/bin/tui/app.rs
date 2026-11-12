//! State of the TUI: what is loaded, what is shown, what is selected.
//!
//! Keys are handled in `input.rs`, what they do is in `actions.rs`, project commands in `commands.rs`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use chrono::prelude::*;
use ratatui::widgets::{ListState, TableState};

use asciii::project::spec::Invoicable;
use asciii::project::Project;
use asciii::stats::{self, Task, YearStats};
use asciii::storage::Storable;

use super::actions::{self, Action, Ctx, PanelRow};
use super::checks;
use super::data::{self, Broken, Data, Entry, Location};
use super::dialogs::{Button, Dialog, Effect, Modal, Tone};
use super::mouse::Hits;
use super::offer_editor::{EditorMode, OfferEditor};
use super::template_editor::{After, TemplateEditor};
use super::ops::{GitOp, Op};
use super::text_input::TextInput;
use super::yaml_edit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Projects,
    Detail,
    Log,
    /// the offer editor of the project in `detail`
    Offer,
    /// catalog and starting offer of a template
    Template,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// projects of the selected year
    Year,
    /// every project, also undated ones
    AllYears,
    /// folders that could not be opened
    Broken,
}

impl Scope {
    pub fn next(self) -> Self {
        match self {
            Scope::Year => Scope::AllYears,
            Scope::AllYears => Scope::Broken,
            Scope::Broken => Scope::Year,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Date,
    /// the bill (invoice) number, within the year of the invoice
    Number,
}

impl SortBy {
    fn next(self) -> Self {
        match self {
            SortBy::Date => SortBy::Number,
            SortBy::Number => SortBy::Date,
        }
    }

    pub fn label(self) -> String {
        match self {
            SortBy::Date => lformat!("date"),
            SortBy::Number => lformat!("bill number"),
        }
    }
}

/// The external programs, looked up once at the start.
#[derive(Debug, Clone)]
pub struct Tools {
    /// `Err(reason)` when PDFs cannot be created
    pub pdf: Result<(), String>,
    pub editor: bool,
    pub git: bool,
}

impl Default for Tools {
    fn default() -> Self {
        Tools {
            pdf: Ok(()),
            editor: true,
            git: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Error,
}

/// A one-line message above the footer.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub since: Instant,
}

/// Why the projects could not be loaded, for the start page.
#[derive(Debug, Clone)]
pub struct LoadProblem {
    pub error: String,
    /// the storage folder and which of its folders are missing; `None` if they all exist
    pub missing: Option<(PathBuf, Vec<String>)>,
}

pub struct App {
    pub screen: Screen,
    /// screen to return to from the project page
    pub previous: Screen,
    pub data: Option<Data>,
    pub loading: Option<Receiver<Result<Data, String>>>,
    pub load_problem: Option<LoadProblem>,
    pub today: Date<Utc>,
    pub year: i32,

    pub stats: YearStats,
    /// to-dos, `Task::index` points into `data.entries`
    pub tasks: Vec<Task>,
    /// indices into `data.entries`
    pub upcoming: Vec<usize>,
    pub task_state: ListState,

    pub scope: Scope,
    /// include archived projects in the list (`a`)
    pub show_archived: bool,
    pub sort: SortBy,
    pub sort_descending: bool,
    pub search: TextInput,
    pub searching: bool,
    /// visible rows: indices into `data.entries` (or `data.broken` in the broken scope)
    pub rows: Vec<usize>,
    pub table: TableState,

    pub detail: Option<usize>,
    pub detail_scroll: u16,
    /// selected row of the Actions panel
    pub panel: usize,
    /// the project page is wide enough for the Actions panel beside the content
    pub wide: bool,
    pub log_scroll: u16,

    pub modal: Option<Modal>,
    pub toast: Option<Toast>,
    pub pending: Option<Op>,
    reselect: Option<PathBuf>,
    /// open this project's page after the next load (a new project)
    open_after_load: Option<PathBuf>,
    pub quit: bool,
    /// quit once the offer is saved (Ctrl+Q → Save and quit)
    pub quit_after_save: bool,

    pub tools: Tools,
    /// mouse reporting on; off lets the terminal select text
    pub mouse: bool,
    /// clickable regions of the last frame
    pub hits: Hits,
    pub last_click: Option<(u16, u16, Instant)>,
    /// focused button of the start page
    pub first_run_focus: usize,

    /// startup checks done (after the first load)
    checked: bool,
    /// background `git fetch` started after the first load
    fetch: Option<Receiver<Result<checks::Remote, String>>>,
    /// what the fetch found, shown in the top bar
    pub remote: Option<checks::Remote>,
    /// popups that arrived while another one was open, shown one after the other
    queued: Vec<Modal>,

    /// the offer editor (`o` on the project page)
    pub offer: Option<OfferEditor>,
    /// the Template tab
    pub template: Option<TemplateEditor>,
}

fn today() -> Date<Utc> {
    Utc::today()
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let today = today();
        App {
            screen: Screen::Dashboard,
            previous: Screen::Projects,
            data: None,
            loading: None,
            load_problem: None,
            today,
            year: today.year(),
            stats: YearStats::default(),
            tasks: Vec::new(),
            upcoming: Vec::new(),
            task_state: ListState::default(),
            scope: Scope::Year,
            show_archived: true,
            sort: SortBy::Date,
            // newest and upcoming projects first
            sort_descending: true,
            search: TextInput::default(),
            searching: false,
            rows: Vec::new(),
            table: TableState::default(),
            detail: None,
            detail_scroll: 0,
            panel: 0,
            wide: true,
            log_scroll: 0,
            modal: None,
            toast: None,
            pending: None,
            reselect: None,
            open_after_load: None,
            quit: false,
            quit_after_save: false,
            tools: Tools::default(),
            mouse: true,
            hits: Hits::default(),
            last_click: None,
            first_run_focus: 0,
            checked: false,
            fetch: None,
            remote: None,
            queued: Vec::new(),
            offer: None,
            template: None,
        }
    }

    /// Where the user is, for the action registry.
    pub fn ctx(&self) -> Ctx {
        match self.screen {
            Screen::Log => Ctx::Log,
            Screen::Template => Ctx::Template,
            _ if self.data.is_none() => Ctx::Empty,
            Screen::Dashboard => Ctx::Dashboard,
            Screen::Projects if self.scope == Scope::Broken => Ctx::Broken,
            Screen::Projects => Ctx::Projects,
            Screen::Detail => Ctx::Detail,
            Screen::Offer if self.offer.as_ref().is_some_and(|e| e.mode == EditorMode::Usage) => Ctx::Usage,
            Screen::Offer => Ctx::Offer,
        }
    }

    // ── loading ────────────────────────────────────────────────────────────

    pub fn start_loading(&mut self, reselect: Option<PathBuf>) {
        if reselect.is_some() {
            self.reselect = reselect;
        } else if self.reselect.is_none() {
            self.reselect = self.selected_entry().map(|e| e.project.file());
        }
        self.loading = Some(data::load_in_background());
    }

    /// After loading, show the page of `file` (a project that was just created).
    pub fn open_after_load(&mut self, file: PathBuf) {
        self.open_after_load = Some(file);
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Picks up the result of the background loader, if it is done.
    pub fn poll_loader(&mut self) {
        self.poll_fetch();
        if self.modal.is_none() && !self.queued.is_empty() {
            self.modal = Some(self.queued.remove(0));
        }
        let Some(receiver) = &self.loading else { return };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(lformat!("loading crashed, see log")),
        };
        self.loading = None;
        match result {
            Ok(data) => {
                log::debug!("loaded {} projects in {:?}", data.entries.len(), data.took);
                self.load_problem = None;
                self.today = today();
                self.data = Some(data);
                self.recompute();
                if !self.checked {
                    self.checked = true;
                    self.startup_checks();
                }
                if let Some(file) = self.open_after_load.take() {
                    if let Some(index) = self.data.as_ref().and_then(|d| d.find_by_file(&file)) {
                        self.screen = Screen::Projects;
                        self.open_project(index, None);
                    }
                }
            },
            Err(error) => {
                log::error!("{}", error);
                self.data = None;
                self.load_problem = Some(LoadProblem {
                    missing: missing_storage(),
                    error,
                });
            },
        }
    }

    /// Recomputes dashboard figures and visible rows after loading or changing year/scope/sort/search.
    pub fn recompute(&mut self) {
        let Some(data) = &self.data else { return };

        let in_year: Vec<&Project> = data
            .entries
            .iter()
            .map(|e| &e.project)
            .filter(|p| stats::in_year(p, self.year))
            .collect();
        self.stats = stats::year_stats(&in_year, self.year, self.today);

        let working: Vec<usize> = (0..data.entries.len())
            .filter(|&i| data.entries[i].location == Location::Working)
            .collect();
        let working_projects: Vec<&Project> = working.iter().map(|&i| &data.entries[i].project).collect();
        // duplicate invoice numbers are a legal problem, they go first; only for projects that are not
        // archived (a working project that clashes with an archived one still shows, naming the archived one)
        let duplicates = working
            .iter()
            .copied()
            .filter(|&i| !data.entries[i].summary.duplicate_of.is_empty())
            .map(|index| stats::Task {
                kind: stats::TaskKind::DuplicateInvoiceNumber,
                index,
                due: None,
                urgent: true,
            });
        self.tasks = duplicates
            .chain(stats::tasks(&working_projects, self.today).into_iter().map(|mut task| {
                task.index = working[task.index];
                task
            }))
            .collect();
        self.upcoming = stats::upcoming(&working_projects, self.today, 30)
            .into_iter()
            .map(|i| working[i])
            .collect();
        if self.task_state.selected().is_none_or(|i| i >= self.tasks.len()) {
            self.task_state.select(if self.tasks.is_empty() { None } else { Some(0) });
        }

        self.refresh_rows();

        if let Some(file) = self.reselect.take() {
            if let Some(index) = self.data.as_ref().and_then(|d| d.find_by_file(&file)) {
                if let Some(row) = self.rows.iter().position(|&r| r == index) {
                    self.table.select(Some(row));
                }
                if self.detail.is_some() {
                    self.detail = Some(index);
                }
            } else if self.screen == Screen::Detail {
                // the project moved (archived, deleted)
                self.detail = None;
                self.screen = self.previous;
            }
        } else if self.detail.is_some_and(|d| self.data.as_ref().is_none_or(|data| d >= data.entries.len())) {
            self.detail = None;
            self.screen = self.previous;
        }
    }

    pub fn refresh_rows(&mut self) {
        let Some(data) = &self.data else { return };
        let selected_file = self.selected_entry().map(|e| e.project.file());

        if self.scope == Scope::Broken {
            self.rows = (0..data.broken.len()).collect();
        } else {
            let needle = self.search.text().to_lowercase();
            let mut rows: Vec<usize> = data
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.show_archived || e.location == Location::Working)
                .filter(|(_, e)| match self.scope {
                    // also projects of the year before whose invoice was written this year
                    Scope::Year => stats::listed_in_year(&e.project, self.year),
                    Scope::AllYears | Scope::Broken => true,
                })
                .filter(|(_, e)| {
                    needle.is_empty()
                        || e.summary.name.to_lowercase().contains(&needle)
                        || e.summary.invoice_number.to_lowercase().contains(&needle)
                        || e.summary.manager.to_lowercase().contains(&needle)
                })
                .map(|(i, _)| i)
                .collect();

            let entries = &data.entries;
            match self.sort {
                SortBy::Date => {
                    rows.sort_by_key(|&i| entries[i].summary.date);
                    if self.sort_descending {
                        rows.reverse();
                    }
                },
                SortBy::Number => {
                    // numbers restart every year, so the year of the invoice comes first;
                    // projects without a bill number stay at the end in both directions
                    let key = |i: usize| {
                        let project = &entries[i].project;
                        project.invoice().number().ok().map(|n| (stats::invoice_year(project), n))
                    };
                    let (mut numbered, unnumbered): (Vec<usize>, Vec<usize>) =
                        rows.into_iter().partition(|&i| key(i).is_some());
                    numbered.sort_by_key(|&i| key(i));
                    if self.sort_descending {
                        numbered.reverse();
                    }
                    numbered.extend(unnumbered);
                    rows = numbered;
                },
            }
            self.rows = rows;
        }

        let row = selected_file
            .filter(|_| self.scope != Scope::Broken)
            .and_then(|file| self.rows.iter().position(|&r| data.entries[r].project.file() == file))
            .or(if self.rows.is_empty() { None } else { Some(0) });
        self.table.select(row);
    }

    // ── accessors ──────────────────────────────────────────────────────────

    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.data.as_ref().and_then(|d| d.entries.get(index))
    }

    /// The project the actions apply to: the project page, the selected row or the selected to-do.
    pub fn selected_entry(&self) -> Option<&Entry> {
        match self.screen {
            Screen::Detail | Screen::Offer => self.detail.and_then(|i| self.entry(i)),
            Screen::Dashboard => self
                .task_state
                .selected()
                .and_then(|t| self.tasks.get(t))
                .and_then(|t| self.entry(t.index)),
            Screen::Log | Screen::Template => None,
            Screen::Projects if self.scope == Scope::Broken => None,
            Screen::Projects => self
                .table
                .selected()
                .and_then(|row| self.rows.get(row))
                .and_then(|&i| self.entry(i)),
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        let entry = self.selected_entry()?;
        self.data.as_ref()?.find_by_file(&entry.project.file())
    }

    /// The selected folder in the broken scope.
    pub fn selected_broken(&self) -> Option<&Broken> {
        if self.screen != Screen::Projects || self.scope != Scope::Broken {
            return None;
        }
        let row = self.table.selected()?;
        self.data.as_ref()?.broken.get(*self.rows.get(row)?)
    }

    fn selected_content(&self) -> Option<String> {
        std::fs::read_to_string(self.selected_entry()?.project.file()).ok()
    }

    /// The fillable fields of the selected project file.
    pub fn selected_fields(&self) -> Vec<(&'static str, yaml_edit::Kind, Option<String>)> {
        self.selected_content().map(|c| yaml_edit::fields_in(&c)).unwrap_or_default()
    }

    pub fn selected_has_field(&self, path: &str) -> bool {
        self.selected_fields().iter().any(|(p, _, _)| *p == path)
    }

    pub fn selected_value(&self, path: &str) -> Option<String> {
        yaml_edit::value_at(&self.selected_content()?, path)
    }

    /// `offer/accepted` is set in the file of `project`.
    pub fn is_accepted(&self, project: &Project) -> bool {
        let content = std::fs::read_to_string(project.file()).unwrap_or_default();
        yaml_edit::value_at(&content, "offer/accepted")
            .is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "yes" | "ja" | "y"))
    }

    /// The storage folder, if loading failed because folders are missing.
    pub fn storage_missing(&self) -> Option<PathBuf> {
        self.load_problem.as_ref()?.missing.as_ref().map(|(root, _)| root.clone())
    }

    // ── messages ───────────────────────────────────────────────────────────

    /// A success message; it goes away after a few seconds.
    pub fn set_status(&mut self, message: impl Into<String>, error: bool) {
        if error {
            self.fail(message);
        } else {
            self.success(message);
        }
    }

    pub fn success(&mut self, message: impl Into<String>) {
        let text = message.into();
        // keep a copy in the log tab
        log::info!("{}", text);
        self.toast = Some(Toast {
            text,
            kind: ToastKind::Success,
            since: Instant::now(),
        });
    }

    /// A hint, e.g. why an action is not available.
    pub fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast {
            text: message.into(),
            kind: ToastKind::Info,
            since: Instant::now(),
        });
    }

    /// An error; it stays until the next key press or click.
    pub fn fail(&mut self, message: impl Into<String>) {
        let text = message.into();
        log::error!("{}", text);
        self.toast = Some(Toast {
            text,
            kind: ToastKind::Error,
            since: Instant::now(),
        });
    }

    /// Success and hints go away after a while, errors wait for a key.
    pub fn expire_toast(&mut self) {
        if let Some(toast) = &self.toast {
            let shown_for = match toast.kind {
                ToastKind::Success => Duration::from_secs(5),
                ToastKind::Info => Duration::from_secs(8),
                ToastKind::Error => return,
            };
            if toast.since.elapsed() >= shown_for {
                self.toast = None;
            }
        }
    }

    /// A key or click was seen after the error: it was read.
    pub fn clear_error(&mut self) {
        // give the user a moment, a key pressed together with the error must not hide it at once
        if self.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Error && t.since.elapsed() > Duration::from_millis(300)) {
            self.toast = None;
        }
    }

    // ── navigation ─────────────────────────────────────────────────────────

    pub fn go(&mut self, screen: Screen) {
        if screen != Screen::Template && self.template_blocks(After::Screen(screen)) {
            return;
        }
        if screen == Screen::Template {
            self.open_template();
        }
        if screen != Screen::Detail {
            self.detail = None;
        }
        self.searching = false;
        self.screen = screen;
    }

    pub fn switch_screen(&mut self, forward: bool) {
        let order = [Screen::Dashboard, Screen::Projects, Screen::Template, Screen::Log];
        let current = order
            .iter()
            .position(|s| *s == self.screen)
            .unwrap_or(if self.previous == Screen::Dashboard { 0 } else { 1 });
        let next = if forward {
            (current + 1) % order.len()
        } else {
            (current + order.len() - 1) % order.len()
        };
        self.go(order[next]);
    }

    /// Another year; the list shows that year then, so the key always has a visible effect.
    pub fn change_year(&mut self, delta: i32) {
        self.year += delta;
        if self.scope != Scope::Year {
            self.scope = Scope::Year;
            self.search.clear();
        }
        self.recompute();
    }

    /// Opens the page of project `index`, with `action` highlighted in the Actions panel (else the next step).
    pub fn open_project(&mut self, index: usize, action: Option<Action>) {
        if self.template_blocks(After::Project(index)) {
            return;
        }
        if !matches!(self.screen, Screen::Detail | Screen::Offer) {
            self.previous = self.screen;
        }
        self.searching = false;
        self.detail = Some(index);
        self.detail_scroll = 0;
        self.screen = Screen::Detail;
        // the row in the list follows, so going back lands on it
        if let Some(row) = self.rows.iter().position(|&r| r == index).filter(|_| self.scope != Scope::Broken) {
            self.table.select(Some(row));
        }
        let rows = actions::panel_rows(self);
        self.panel = action
            .and_then(|a| rows.iter().position(|r| r.action() == Some(a)))
            .or_else(|| rows.iter().position(|r| r.action().is_some_and(|a| a.availability(self).is_enabled())))
            .unwrap_or(0);
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
        self.screen = self.previous;
    }

    /// Esc: one step back. Never quits.
    pub fn back(&mut self) {
        match self.screen {
            Screen::Offer => self.leave_offer(),
            Screen::Detail => self.close_detail(),
            Screen::Template => {
                let Some(editor) = self.template.as_mut() else { return };
                if editor.searching || !editor.search.is_empty() {
                    editor.searching = false;
                    editor.search.clear();
                    let any = !editor.rows().is_empty();
                    editor.table.select(any.then_some(0));
                } else {
                    self.template_blocks(After::Stay);
                }
            },
            Screen::Projects if self.searching || !self.search.is_empty() => {
                self.searching = false;
                self.search.clear();
                self.refresh_rows();
            },
            _ => {},
        }
    }

    /// Ctrl+Q, Ctrl+C and `q` on the tabs. Unsaved offer changes are not lost without asking.
    pub fn request_quit(&mut self) {
        if self.template_blocks(After::Quit) {
            return;
        }
        match self.offer.as_ref().filter(|e| e.dirty) {
            Some(editor) => {
                let name = editor.name.clone();
                self.modal = Some(Modal::Dialog(Dialog::with(
                    lformat!("Unsaved changes"),
                    vec![lformat!("The offer of {} has changes that are not saved yet.", name)],
                    Tone::Warning,
                    vec![
                        Button::new(lformat!("Save and quit"), Some('s'), Effect::QuitSave),
                        Button::new(lformat!("Quit without saving"), Some('d'), Effect::Quit),
                        Button::new(lformat!("Keep editing"), Some('k'), Effect::Close),
                    ],
                    0,
                    2,
                )));
            },
            None => self.quit = true,
        }
    }

    pub fn toggle_mouse(&mut self) {
        self.mouse = !self.mouse;
        self.notify(if self.mouse {
            lformat!("mouse on: click and scroll; Shift-drag still selects text")
        } else {
            lformat!("mouse off: the terminal selects text as usual; Ctrl+P → Mouse turns it on again")
        });
    }

    pub fn set_scope(&mut self, scope: Scope) {
        if scope != self.scope {
            self.scope = scope;
            // a search does not stay active unseen
            self.search.clear();
            self.searching = false;
        }
        self.refresh_rows();
    }

    /// The project list in `scope`.
    pub fn show_scope(&mut self, scope: Scope) {
        self.go(Screen::Projects);
        self.set_scope(scope);
    }

    pub fn toggle_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.refresh_rows();
    }

    /// `s`: ascending, descending, then the other sorting (date or bill number).
    pub fn next_sort(&mut self) {
        if self.sort_descending {
            self.sort = self.sort.next();
            self.sort_descending = false;
        } else {
            self.sort_descending = true;
        }
        self.refresh_rows();
    }

    /// A click on a column header: sort by it, again reverses.
    pub fn sort_by(&mut self, sort: SortBy) {
        if self.sort == sort {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort = sort;
            self.sort_descending = false;
        }
        self.refresh_rows();
    }

    pub fn start_search(&mut self) {
        self.searching = true;
    }

    /// Enter on the dashboard or in the list.
    pub fn open_selected(&mut self) {
        match self.screen {
            Screen::Dashboard => {
                let Some(task) = self.task_state.selected().and_then(|t| self.tasks.get(t)).cloned() else { return };
                // the page opens with the action of the to-do highlighted
                let action = self.task_action(&task);
                self.open_project(task.index, action);
            },
            Screen::Projects if self.scope == Scope::Broken => self.show_broken(),
            Screen::Projects => {
                if let Some(index) = self.selected_index() {
                    self.open_project(index, None);
                }
            },
            _ => {},
        }
    }

    // ── the Actions panel ──────────────────────────────────────────────────

    /// Enter on the project page: runs the highlighted action, or opens the panel as a popup when it is hidden.
    pub fn show_actions(&mut self) {
        if self.wide {
            self.run_panel();
        } else {
            self.modal = Some(Modal::Actions);
        }
    }

    /// Runs the highlighted row of the Actions panel.
    pub fn run_panel(&mut self) {
        let rows = actions::panel_rows(self);
        match rows.get(self.panel) {
            Some(PanelRow::Next { action: None, text }) => {
                let text = text.clone();
                self.notify(lformat!("nothing to do now: {}", text));
            },
            Some(row) => {
                if let Some(action) = row.action() {
                    self.run(action);
                }
            },
            None => {},
        }
    }

    /// ↑/↓ in the Actions panel, over the group headings.
    pub fn move_panel(&mut self, delta: isize) {
        let rows = actions::panel_rows(self);
        let mut index = self.panel.min(rows.len().saturating_sub(1)) as isize;
        loop {
            index += delta;
            if index < 0 || index as usize >= rows.len() {
                return;
            }
            if rows[index as usize].selectable() {
                self.panel = index as usize;
                return;
            }
        }
    }

    fn on_actions_popup_key_inner(&mut self, key: &ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return false,
            KeyCode::Up | KeyCode::Char('k') => self.move_panel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_panel(1),
            KeyCode::Enter => {
                self.run_panel();
                return false;
            },
            _ => {
                // the action keys work in the popup, too
                if let Some(action) = actions::for_key(Ctx::Detail, key).filter(|a| *a != Action::Back) {
                    self.run(action);
                    return false;
                }
            },
        }
        true
    }

    pub fn on_actions_popup_key(&mut self, key: &ratatui::crossterm::event::KeyEvent) -> Option<Modal> {
        self.on_actions_popup_key_inner(key).then_some(Modal::Actions)
    }

    // ── popups ─────────────────────────────────────────────────────────────

    pub fn open_help(&mut self) {
        self.modal = Some(Modal::Help { scroll: 0 });
    }

    /// Opens `modal`, or queues it while another popup is open.
    pub fn show(&mut self, modal: Modal) {
        if self.modal.is_none() {
            self.modal = Some(modal);
        } else {
            self.queued.push(modal);
        }
    }

    // ── startup ────────────────────────────────────────────────────────────

    /// Programs and git: everything goes to the log, problems also into a popup that says what to do.
    fn startup_checks(&mut self) {
        let mut results = checks::programs(&super::ops::editor());
        self.tools = checks::tools(&results);
        if let Some(data) = &self.data {
            let root = super::paths::tilde(&data.root);
            if data.git {
                self.fetch = Some(checks::fetch_in_background(data.root.clone()));
                results.push(checks::Check {
                    ok: true,
                    program: None,
                    text: lformat!("{} is a git repository, checking the remote …", root),
                    fix: None,
                });
            } else {
                results.push(checks::Check {
                    ok: false,
                    program: None,
                    text: lformat!("{} is not a git repository: no history, no backup, no sharing.", root),
                    fix: Some(lformat!("set it up with `git init` in that folder, or get the shared one with `asciii clone`")),
                });
            }
        }

        for check in &results {
            if check.ok {
                log::info!("{}", check.text);
            } else {
                log::warn!("{}", check.text);
            }
        }
        let problems: Vec<&checks::Check> = results.iter().filter(|c| !c.ok).collect();
        if !problems.is_empty() {
            let mut lines = Vec::new();
            for problem in problems {
                lines.push(format!("⚠ {}", problem.text));
                if let Some(fix) = &problem.fix {
                    lines.push(format!("  → {}", fix));
                }
            }
            lines.push(String::new());
            lines.push(lformat!("Everything else works. Actions that need a missing program are shown greyed out with the reason."));
            self.show(Modal::Dialog(Dialog::notice(lformat!("Startup checks"), lines, Tone::Warning)));
        }
    }

    /// Result of the background `git fetch`: offer to pull when the remote is ahead.
    fn poll_fetch(&mut self) {
        let Some(receiver) = &self.fetch else { return };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(String::from("git fetch crashed")),
        };
        self.fetch = None;
        match result {
            Ok(remote) => {
                match remote {
                    checks::Remote::Compared { behind, ahead } => {
                        log::info!("{}", lformat!("git remote: {} new, {} not pushed", behind, ahead));
                        if behind > 0 {
                            self.show(Modal::Dialog(Dialog::confirm(
                                lformat!("New changes on the remote"),
                                vec![
                                    lformat!("The remote has {} commit(s) you do not have yet.", behind),
                                    lformat!("Pull them before changing projects, otherwise you may run into conflicts."),
                                ],
                                lformat!("Pull now"),
                                Effect::Op(Op::Git(GitOp::Pull)),
                                false,
                            )));
                        } else if ahead > 0 {
                            self.notify(lformat!("{} local commit(s) not pushed yet (g → push)", ahead));
                        }
                    },
                    checks::Remote::NoUpstream => {
                        log::warn!("{}", lformat!("the git branch has no upstream, nothing to compare with"));
                    },
                }
                self.remote = Some(remote);
            },
            Err(error) => {
                log::warn!("{}", lformat!("git fetch failed: {}", error));
                self.notify(lformat!("git fetch failed: {}", error));
            },
        }
    }

    /// Compares with the git remote again (after pull, push or commit).
    pub fn check_remote(&mut self) {
        if let Some(data) = self.data.as_ref().filter(|d| d.git) {
            self.remote = None;
            self.fetch = Some(checks::fetch_in_background(data.root.clone()));
        }
    }
}

/// The storage folder and which of root, working, archive, templates are missing; `None` if none is.
fn missing_storage() -> Option<(PathBuf, Vec<String>)> {
    let root = super::paths::storage()?.to_owned();
    let mut missing = Vec::new();
    if !root.is_dir() {
        missing.push(super::paths::tilde(&root));
    }
    for key in ["dirs/working", "dirs/archive", "dirs/templates"] {
        let name = asciii::CONFIG.get_str_or(key).unwrap_or_default();
        if !name.is_empty() && !root.join(name).is_dir() {
            missing.push(format!("{}/", name));
        }
    }
    (!missing.is_empty()).then_some((root, missing))
}
