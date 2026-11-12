//! Mouse: drawing registers what is clickable where, a click goes to the topmost region under the pointer.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::actions::{Action, PanelRow};
use super::app::{App, Scope, Screen, SortBy};
use super::dialogs::Modal;
use super::forms::FieldKind;

/// What a click on a region does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Action(Action),
    Tab(Screen),
    TaskRow(usize),
    UpcomingRow(usize),
    /// row of the project list (index into `rows`)
    ProjectRow(usize),
    /// row of the Actions panel (index into its rows)
    PanelRow(usize),
    PaletteRow(usize),
    PickRow(usize),
    CatalogRow(usize),
    CatalogButton(usize),
    /// button of a dialog, the typed confirmation (1, 2) or the first-run page
    Button(usize),
    FormField(usize),
    FormButton(usize),
    TypedInput,
    Header(SortBy),
    Scope(Scope),
    OfferRow(usize),
    /// row of the Template tab's table
    TemplateRow(usize),
    /// inside a popup, nothing to do
    Inside,
}

/// What the wheel scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Tasks,
    Projects,
    Detail,
    Panel,
    Log,
    Output,
    Help,
    Palette,
    Pick,
    Offer,
    Template,
    Form,
}

#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub rect: Rect,
    pub target: Target,
}

/// Regions of the last drawn frame.
#[derive(Debug, Default)]
pub struct Hits {
    pub clicks: Vec<Hit>,
    pub wheels: Vec<(Rect, Wheel)>,
    /// regions from these indices on belong to the popup
    modal: Option<(usize, usize, Rect)>,
}

impl Hits {
    pub fn clear(&mut self) {
        self.clicks.clear();
        self.wheels.clear();
        self.modal = None;
    }

    pub fn click(&mut self, rect: Rect, target: Target) {
        if rect.width > 0 && rect.height > 0 {
            self.clicks.push(Hit { rect, target });
        }
    }

    pub fn wheel(&mut self, rect: Rect, wheel: Wheel) {
        self.wheels.push((rect, wheel));
    }

    /// From now on regions belong to the popup drawn in `rect`.
    pub fn start_modal(&mut self, rect: Rect) {
        self.modal = Some((self.clicks.len(), self.wheels.len(), rect));
    }

    pub fn modal_rect(&self) -> Option<Rect> {
        self.modal.map(|(_, _, rect)| rect)
    }

    /// The topmost click target at `column`, `row`. With a popup open only its regions count.
    pub fn target_at(&self, column: u16, row: u16) -> Option<Target> {
        let at = Position::new(column, row);
        let from = self.modal.map_or(0, |(clicks, _, _)| clicks);
        if let Some(hit) = self.clicks[from..].iter().rev().find(|h| h.rect.contains(at)) {
            return Some(hit.target);
        }
        match self.modal {
            Some((_, _, rect)) if rect.contains(at) => Some(Target::Inside),
            _ => None,
        }
    }

    pub fn wheel_at(&self, column: u16, row: u16) -> Option<Wheel> {
        let at = Position::new(column, row);
        let from = self.modal.map_or(0, |(_, wheels, _)| wheels);
        self.wheels[from..].iter().rev().find(|(r, _)| r.contains(at)).map(|(_, w)| *w)
    }
}

const WHEEL_STEP: u16 = 3;

impl App {
    pub fn on_mouse(&mut self, event: MouseEvent) {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.clear_error();
                let double = self
                    .last_click
                    .is_some_and(|(c, r, at)| c == event.column && r == event.row && at.elapsed() < Duration::from_millis(400));
                self.last_click = Some((event.column, event.row, Instant::now()));
                match self.hits.target_at(event.column, event.row) {
                    Some(target) => self.click(target, double),
                    // outside a popup: harmless ones close, the others ignore it
                    None if self.modal.as_ref().is_some_and(Modal::harmless) => self.modal = None,
                    None => {},
                }
            },
            MouseEventKind::ScrollDown => self.wheel(event.column, event.row, true),
            MouseEventKind::ScrollUp => self.wheel(event.column, event.row, false),
            _ => {},
        }
    }

    fn wheel(&mut self, column: u16, row: u16, down: bool) {
        let Some(wheel) = self.hits.wheel_at(column, row) else { return };
        let step = |value: u16| if down { value.saturating_add(WHEEL_STEP) } else { value.saturating_sub(WHEEL_STEP) };
        let step_index = |value: usize, count: usize| {
            if down {
                (value + WHEEL_STEP as usize).min(count.saturating_sub(1))
            } else {
                value.saturating_sub(WHEEL_STEP as usize)
            }
        };
        match wheel {
            Wheel::Tasks => {
                let selected = self.task_state.selected().unwrap_or(0);
                if !self.tasks.is_empty() {
                    self.task_state.select(Some(step_index(selected, self.tasks.len())));
                }
            },
            Wheel::Projects => {
                let selected = self.table.selected().unwrap_or(0);
                if !self.rows.is_empty() {
                    self.table.select(Some(step_index(selected, self.rows.len())));
                }
            },
            Wheel::Detail => self.detail_scroll = step(self.detail_scroll),
            // the log counts from the newest line
            Wheel::Log => {
                self.log_scroll = if down { self.log_scroll.saturating_sub(WHEEL_STEP) } else { self.log_scroll.saturating_add(WHEEL_STEP) }
            },
            Wheel::Panel => self.move_panel(if down { 1 } else { -1 }),
            Wheel::Offer => {
                if let Some(editor) = self.offer.as_mut() {
                    let count = editor.offer.lines.len();
                    let selected = editor.table.selected().unwrap_or(0);
                    if count > 0 {
                        editor.table.select(Some(step_index(selected, count)));
                    }
                }
            },
            Wheel::Template => {
                if let Some(editor) = self.template.as_mut() {
                    let count = editor.rows().len();
                    let selected = editor.table.selected().unwrap_or(0);
                    if count > 0 {
                        editor.table.select(Some(step_index(selected, count)));
                    }
                }
            },
            Wheel::Help | Wheel::Output | Wheel::Palette | Wheel::Pick | Wheel::Form => match self.modal.as_mut() {
                Some(Modal::Help { scroll }) | Some(Modal::Output { scroll, .. }) => *scroll = step(*scroll),
                Some(Modal::Form(form)) => form.scroll = step(form.scroll),
                Some(Modal::Palette(palette)) => {
                    palette.selected = if down { palette.selected + 1 } else { palette.selected.saturating_sub(1) }
                },
                Some(Modal::Pick(pick)) => {
                    let count = pick.items.len();
                    let selected = pick.state.selected().unwrap_or(0);
                    pick.state.select(Some(if down { (selected + 1).min(count.saturating_sub(1)) } else { selected.saturating_sub(1) }));
                },
                _ => {},
            },
        }
    }

    fn click(&mut self, target: Target, double: bool) {
        match target {
            Target::Inside => {},
            Target::Action(action) => self.run(action),
            Target::Tab(screen) => {
                if self.screen == Screen::Offer && screen != Screen::Projects {
                    // leaving the editor goes through its unsaved-changes question
                    self.run(Action::Back);
                } else if screen == Screen::Projects && matches!(self.screen, Screen::Detail | Screen::Offer) {
                    // already there
                } else {
                    self.go(screen);
                }
            },
            Target::TaskRow(index) => {
                let again = self.task_state.selected() == Some(index);
                self.task_state.select(Some(index));
                if again || double {
                    self.run(Action::Open);
                }
            },
            Target::UpcomingRow(index) => {
                if let Some(&entry) = self.upcoming.get(index) {
                    self.open_project(entry, None);
                }
            },
            Target::ProjectRow(index) => {
                let again = self.table.selected() == Some(index);
                self.table.select(Some(index));
                if again || double {
                    self.run(Action::Open);
                }
            },
            Target::PanelRow(index) => {
                let rows = super::actions::panel_rows(self);
                if rows.get(index).is_some_and(PanelRow::selectable) {
                    self.panel = index;
                    if matches!(self.modal, Some(Modal::Actions)) {
                        self.modal = None;
                    }
                    self.run_panel();
                }
            },
            Target::Header(sort) => self.sort_by(sort),
            Target::Scope(scope) => self.set_scope(scope),
            Target::OfferRow(index) => {
                let Some(editor) = self.offer.as_mut() else { return };
                let again = editor.table.selected() == Some(index);
                editor.table.select(Some(index));
                if again || double {
                    self.run(Action::ChangeLine);
                }
            },
            Target::TemplateRow(index) => {
                let Some(editor) = self.template.as_mut() else { return };
                let again = editor.table.selected() == Some(index);
                editor.table.select(Some(index));
                if again || double {
                    self.run(Action::ChangeLine);
                }
            },
            Target::PaletteRow(index) => {
                if let Some(Modal::Palette(palette)) = self.modal.take() {
                    let rest = self.run_palette_row(palette, Some(index));
                    self.keep_modal(rest);
                }
            },
            Target::PickRow(index) => {
                if let Some(Modal::Pick(mut pick)) = self.modal.take() {
                    pick.state.select(Some(index));
                    let rest = self.choose(pick, index);
                    self.keep_modal(rest);
                }
            },
            Target::CatalogRow(index) => {
                if let Some(Modal::CatalogSync { changes, state, focus, .. }) = self.modal.as_mut() {
                    state.select(Some(index));
                    *focus = 0;
                    if let Some(change) = changes.get_mut(index) {
                        change.1 = !change.1;
                    }
                }
            },
            Target::CatalogButton(button) => {
                if let Some(Modal::CatalogSync { template, changes, .. }) = self.modal.take() {
                    if button == 1 {
                        self.write_catalog(template, changes);
                    } else {
                        self.notify(lformat!("template not changed"));
                    }
                }
            },
            Target::Button(index) => match self.modal.take() {
                Some(Modal::Dialog(dialog)) => {
                    let rest = self.press(dialog, Some(index));
                    self.keep_modal(rest);
                },
                Some(Modal::Typed(mut typed)) => {
                    typed.focus = index;
                    let rest = self.on_modal_typed_enter(typed);
                    self.keep_modal(rest);
                },
                other => {
                    self.modal = other;
                    if self.modal.is_none() {
                        self.press_first_run(index);
                    }
                },
            },
            Target::TypedInput => {
                if let Some(Modal::Typed(typed)) = self.modal.as_mut() {
                    typed.focus = 0;
                }
            },
            Target::FormField(index) => {
                if let Some(Modal::Form(form)) = self.modal.as_mut() {
                    if form.focus != index {
                        form.set_focus(index);
                    }
                    // a click on a choice or toggle also changes it
                    if let Some(field) = form.fields.get_mut(index) {
                        match (&field.kind, &field.locked) {
                            (FieldKind::Choice(_), _) | (FieldKind::Toggle, None) => field.cycle(1),
                            (FieldKind::Toggle, Some(reason)) => {
                                let reason = reason.clone();
                                self.notify(reason);
                            },
                            _ => {},
                        }
                    }
                }
            },
            Target::FormButton(index) => {
                if let Some(Modal::Form(form)) = self.modal.take() {
                    let rest = if index == 0 { self.submit_form(form) } else { None };
                    self.keep_modal(rest);
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topmost_region_wins_and_popups_block_the_rest() {
        let mut hits = Hits::default();
        hits.click(Rect::new(0, 0, 80, 24), Target::ProjectRow(0));
        hits.click(Rect::new(10, 5, 10, 1), Target::ProjectRow(3));
        assert_eq!(hits.target_at(12, 5), Some(Target::ProjectRow(3)));
        assert_eq!(hits.target_at(2, 2), Some(Target::ProjectRow(0)));

        hits.start_modal(Rect::new(20, 8, 40, 8));
        hits.click(Rect::new(22, 14, 8, 1), Target::Button(0));
        assert_eq!(hits.target_at(23, 14), Some(Target::Button(0)));
        // inside the popup but on nothing: swallowed
        assert_eq!(hits.target_at(40, 10), Some(Target::Inside));
        // outside the popup: nothing below it is reached
        assert_eq!(hits.target_at(12, 5), None);
        // empty regions are not registered
        hits.click(Rect::new(0, 0, 0, 5), Target::Button(9));
        assert_eq!(hits.clicks.len(), 3);
    }

    #[test]
    fn wheel_regions() {
        let mut hits = Hits::default();
        hits.wheel(Rect::new(0, 1, 60, 20), Wheel::Detail);
        hits.wheel(Rect::new(60, 1, 20, 20), Wheel::Panel);
        assert_eq!(hits.wheel_at(70, 3), Some(Wheel::Panel));
        assert_eq!(hits.wheel_at(10, 3), Some(Wheel::Detail));
        hits.start_modal(Rect::new(5, 5, 20, 5));
        hits.wheel(Rect::new(5, 5, 20, 5), Wheel::Help);
        assert_eq!(hits.wheel_at(10, 6), Some(Wheel::Help));
        assert_eq!(hits.wheel_at(70, 3), None);
    }
}
