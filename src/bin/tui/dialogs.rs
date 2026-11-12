//! Popups: dialogs with buttons, pick lists, the typed delete confirmation, help, output, catalog sync.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use asciii::project::BillType;

use super::actions::Action;
use super::app::App;
use super::forms::Form;
use super::offer;
use super::ops::{GitOp, Op};
use super::palette::Palette;
use super::template_editor::{After, Row as TemplateRow};
use super::text_input::TextInput;

pub enum Modal {
    Help { scroll: u16 },
    Palette(Palette),
    Dialog(Dialog),
    Typed(Typed),
    Form(Form),
    Pick(Pick),
    Output { title: String, text: String, scroll: u16 },
    /// catalog changes of a project, each can be written to the template or not; `focus`: 0 list, 1 write, 2 keep
    CatalogSync { template: PathBuf, changes: Vec<(offer::Change, bool)>, state: ListState, focus: usize },
    /// the Actions panel of the project page as a popup, on narrow terminals
    Actions,
}

impl Modal {
    /// Popups that lose nothing when closed: a click outside closes them.
    pub fn harmless(&self) -> bool {
        match self {
            Modal::Help { .. } | Modal::Palette(_) | Modal::Output { .. } | Modal::Pick(_) | Modal::Actions => true,
            Modal::Dialog(dialog) => dialog.buttons.iter().all(|b| matches!(b.effect, Effect::Close)),
            _ => false,
        }
    }
}

/// How a dialog looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Warning,
    Danger,
    Success,
}

/// What a button does.
#[derive(Debug, Clone)]
pub enum Effect {
    Close,
    Op(Op),
    Action(Action),
    /// create the invoice although its numbers are used elsewhere
    MakeIgnoringDuplicates { file: PathBuf, bill_type: BillType },
    /// ask for the archive year
    ArchiveForm { dir: PathBuf, name: String, year: String, note: String },
    OfferSave,
    OfferDiscard,
    QuitSave,
    Quit,
    OpenPath(PathBuf),
    CreateStorage,
    TemplateSave(After),
    TemplateDiscard(After),
    TemplateRemove(TemplateRow),
}

#[derive(Debug, Clone)]
pub struct Button {
    pub label: String,
    /// letter that presses it, besides Enter on the focused button
    pub key: Option<char>,
    pub effect: Effect,
}

impl Button {
    pub fn new(label: String, key: Option<char>, effect: Effect) -> Self {
        Button { label, key, effect }
    }
}

pub struct Dialog {
    pub title: String,
    pub lines: Vec<String>,
    pub tone: Tone,
    pub buttons: Vec<Button>,
    pub focus: usize,
    /// the button Esc presses
    pub cancel: usize,
}

impl Dialog {
    /// A question with `[ yes ] [ No ]`. Irreversible things start on "No".
    pub fn confirm(title: String, lines: Vec<String>, yes: String, effect: Effect, irreversible: bool) -> Self {
        Dialog {
            title,
            lines,
            tone: if irreversible { Tone::Warning } else { Tone::Info },
            buttons: vec![
                Button::new(yes, Some('y'), effect),
                Button::new(lformat!("No"), Some('n'), Effect::Close),
            ],
            focus: if irreversible { 1 } else { 0 },
            cancel: 1,
        }
    }

    /// Information with `[ OK ]`.
    pub fn notice(title: String, lines: Vec<String>, tone: Tone) -> Self {
        Dialog {
            title,
            lines,
            tone,
            buttons: vec![Button::new(lformat!("OK"), None, Effect::Close)],
            focus: 0,
            cancel: 0,
        }
    }

    /// A dialog with any buttons; Esc presses `cancel`, the dialog starts on `focus`.
    pub fn with(title: String, lines: Vec<String>, tone: Tone, buttons: Vec<Button>, focus: usize, cancel: usize) -> Self {
        Dialog {
            title,
            lines,
            tone,
            buttons,
            focus,
            cancel,
        }
    }
}

/// Delete confirmation: the project name has to be typed.
pub struct Typed {
    pub message: String,
    pub expected: String,
    pub input: TextInput,
    pub op: Op,
    pub error: Option<String>,
    /// 0 the text field, 1 Delete, 2 Cancel
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub enum PickPurpose {
    /// add a product to the offer in the editor
    OfferAdd(Vec<offer::CatalogItem>),
    /// pick the document to create
    Make(PathBuf),
    Git,
}

#[derive(Debug, Clone)]
pub struct PickItem {
    pub label: String,
    /// why it cannot be chosen
    pub disabled: Option<String>,
}

pub struct Pick {
    pub title: String,
    pub items: Vec<PickItem>,
    pub state: ListState,
    pub purpose: PickPurpose,
}

impl Pick {
    pub fn new(title: String, items: Vec<PickItem>, selected: usize, purpose: PickPurpose) -> Self {
        Pick {
            title,
            items,
            state: ListState::default().with_selected(Some(selected)),
            purpose,
        }
    }
}

/// Scrolls `scroll` for the movement keys of a text viewer. `None` for other keys.
pub fn scroll_key(scroll: u16, key: &KeyEvent) -> Option<u16> {
    Some(match key.code {
        KeyCode::Down | KeyCode::Char('j') => scroll.saturating_add(1),
        KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
        KeyCode::PageDown | KeyCode::Char(' ') => scroll.saturating_add(15),
        KeyCode::PageUp => scroll.saturating_sub(15),
        KeyCode::Home => 0,
        KeyCode::End => u16::MAX,
        _ => return None,
    })
}

impl App {
    pub fn on_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.modal.take() else { return };
        let rest = match modal {
            Modal::Help { scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => None,
                _ => Some(Modal::Help {
                    scroll: scroll_key(scroll, &key).unwrap_or(scroll),
                }),
            },
            Modal::Output { title, text, scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => None,
                _ => Some(Modal::Output {
                    title,
                    text,
                    scroll: scroll_key(scroll, &key).unwrap_or(scroll),
                }),
            },
            Modal::Palette(palette) => self.on_palette_key(palette, &key),
            Modal::Form(form) => self.on_form_key(form, &key),
            Modal::Dialog(dialog) => self.on_dialog_key(dialog, &key),
            Modal::Typed(typed) => self.on_typed_key(typed, &key),
            Modal::Pick(pick) => self.on_pick_key(pick, &key),
            Modal::CatalogSync {
                template,
                changes,
                state,
                focus,
            } => self.on_catalog_key(template, changes, state, focus, &key),
            Modal::Actions => self.on_actions_popup_key(&key),
        };
        self.keep_modal(rest);
    }

    /// Puts back what is left of the popup, unless handling it opened another one.
    pub fn keep_modal(&mut self, rest: Option<Modal>) {
        if self.modal.is_none() {
            self.modal = rest;
        }
    }

    fn on_dialog_key(&mut self, mut dialog: Dialog, key: &KeyEvent) -> Option<Modal> {
        let count = dialog.buttons.len();
        match key.code {
            KeyCode::Left | KeyCode::BackTab | KeyCode::Up => dialog.focus = (dialog.focus + count - 1) % count,
            KeyCode::Right | KeyCode::Tab | KeyCode::Down => dialog.focus = (dialog.focus + 1) % count,
            KeyCode::Enter | KeyCode::Char(' ') => return self.press(dialog, None),
            KeyCode::Esc => {
                let cancel = dialog.cancel;
                return self.press(dialog, Some(cancel));
            },
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(index) = dialog.buttons.iter().position(|b| b.key.is_some_and(|k| k.eq_ignore_ascii_case(&c))) {
                    return self.press(dialog, Some(index));
                }
            },
            _ => {},
        }
        Some(Modal::Dialog(dialog))
    }

    /// Presses button `index` (the focused one if `None`); the dialog closes.
    pub fn press(&mut self, dialog: Dialog, index: Option<usize>) -> Option<Modal> {
        let index = index.unwrap_or(dialog.focus);
        let effect = dialog.buttons.into_iter().nth(index).map(|b| b.effect)?;
        self.apply(effect);
        None
    }

    pub fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::Close => {},
            Effect::Op(op) => self.pending = Some(op),
            Effect::Action(action) => self.run(action),
            Effect::MakeIgnoringDuplicates { file, bill_type } => {
                let next = self.request_make(file, bill_type, true);
                self.keep_modal(next);
            },
            Effect::ArchiveForm { dir, name, year, note } => self.archive_form(dir, name, year, note),
            Effect::OfferSave => self.save_offer(),
            Effect::OfferDiscard => {
                self.close_offer();
                self.notify(lformat!("changes discarded"));
            },
            Effect::QuitSave => {
                self.quit_after_save = true;
                self.save_offer();
                if self.pending.is_none() {
                    self.quit = true;
                }
            },
            Effect::Quit => self.quit = true,
            Effect::OpenPath(path) => {
                if let Err(error) = open::that_detached(&path) {
                    self.fail(lformat!("could not open {}: {}", super::paths::short(&path), error));
                }
            },
            Effect::CreateStorage => self.create_storage(),
            Effect::TemplateSave(after) => self.save_template(after),
            Effect::TemplateDiscard(after) => self.discard_template(after),
            Effect::TemplateRemove(row) => self.template_remove(row),
        }
    }

    fn on_typed_key(&mut self, mut typed: Typed, key: &KeyEvent) -> Option<Modal> {
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Tab | KeyCode::Down => typed.focus = (typed.focus + 1) % 3,
            KeyCode::BackTab | KeyCode::Up => typed.focus = (typed.focus + 2) % 3,
            KeyCode::Left | KeyCode::Right if typed.focus > 0 => typed.focus = 3 - typed.focus,
            KeyCode::Enter => return self.on_modal_typed_enter(typed),
            _ if typed.focus == 0 => {
                if typed.input.on_key(key) {
                    typed.error = None;
                }
            },
            _ => {},
        }
        Some(Modal::Typed(typed))
    }

    /// Enter (or a click) on the focused part of the delete confirmation. A wrong name keeps it open.
    pub fn on_modal_typed_enter(&mut self, mut typed: Typed) -> Option<Modal> {
        if typed.focus == 2 {
            return None;
        }
        if typed.input.text().trim() == typed.expected {
            self.pending = Some(typed.op);
            return None;
        }
        typed.error = Some(lformat!("that is not the name, type \"{}\" exactly", typed.expected));
        typed.focus = 0;
        Some(Modal::Typed(typed))
    }

    fn on_pick_key(&mut self, mut pick: Pick, key: &KeyEvent) -> Option<Modal> {
        let count = pick.items.len();
        let selected = pick.state.selected().unwrap_or(0);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return None,
            KeyCode::Down | KeyCode::Char('j') if count > 0 => pick.state.select(Some((selected + 1).min(count - 1))),
            KeyCode::Up | KeyCode::Char('k') => pick.state.select(Some(selected.saturating_sub(1))),
            KeyCode::Home => pick.state.select(Some(0)),
            KeyCode::End if count > 0 => pick.state.select(Some(count - 1)),
            KeyCode::Enter => return self.choose(pick, selected),
            _ => {},
        }
        Some(Modal::Pick(pick))
    }

    /// Chooses item `index` of a pick list.
    pub fn choose(&mut self, pick: Pick, index: usize) -> Option<Modal> {
        let Some(item) = pick.items.get(index) else { return Some(Modal::Pick(pick)) };
        if let Some(reason) = &item.disabled {
            self.notify(lformat!("{}: {}", item.label, reason));
            return Some(Modal::Pick(pick));
        }
        match pick.purpose {
            PickPurpose::OfferAdd(items) => {
                if let Some(item) = items.get(index) {
                    self.add_picked(item);
                }
                None
            },
            PickPurpose::Make(file) => {
                let bill_type = if index == 0 { BillType::Offer } else { BillType::Invoice };
                self.request_make(file, bill_type, false)
            },
            PickPurpose::Git => {
                let op = *GitOp::ALL.get(index)?;
                self.git(op);
                None
            },
        }
    }

    fn on_catalog_key(
        &mut self,
        template: PathBuf,
        mut changes: Vec<(offer::Change, bool)>,
        mut state: ListState,
        mut focus: usize,
        key: &KeyEvent,
    ) -> Option<Modal> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if focus == 0 => state.select_next(),
            KeyCode::Up | KeyCode::Char('k') if focus == 0 => state.select_previous(),
            KeyCode::Tab => focus = (focus + 1) % 3,
            KeyCode::BackTab => focus = (focus + 2) % 3,
            KeyCode::Left | KeyCode::Right if focus > 0 => focus = 3 - focus,
            KeyCode::Char(' ') if focus == 0 => {
                if let Some(change) = state.selected().and_then(|i| changes.get_mut(i)) {
                    change.1 = !change.1;
                }
            },
            KeyCode::Enter | KeyCode::Char('y') if focus != 2 => {
                self.write_catalog(template, changes);
                return None;
            },
            // special prices stay in this project
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                self.notify(lformat!("template not changed"));
                return None;
            },
            _ => {},
        }
        Some(Modal::CatalogSync {
            template,
            changes,
            state,
            focus,
        })
    }

    pub fn write_catalog(&mut self, template: PathBuf, changes: Vec<(offer::Change, bool)>) {
        let items: Vec<offer::CatalogItem> = changes.into_iter().filter(|(_, chosen)| *chosen).map(|(c, _)| c.item).collect();
        if items.is_empty() {
            self.notify(lformat!("template not changed"));
        } else {
            self.pending = Some(Op::SyncTemplate { template, items });
        }
    }
}
