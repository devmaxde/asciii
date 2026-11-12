//! The offer and usage editor: a copy of the products that is written to the file on save.

use std::path::PathBuf;

use ratatui::widgets::TableState;

use asciii::storage::Storable;
use asciii::CONFIG;

use super::app::{App, Screen};
use super::data::Location;
use super::dialogs::{Button, Dialog, Effect, Modal, Pick, PickItem, PickPurpose, Tone};
use super::forms::{Field, FieldKind, Form, FormPurpose};
use super::offer::{self, Offer};
use super::ops::Op;
use super::yaml_edit;

/// What the editor changes: the offered amounts (`o`) or what was actually used (`u`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Offer,
    Usage,
}

pub struct OfferEditor {
    pub mode: EditorMode,
    pub file: PathBuf,
    pub name: String,
    /// the file as it was when the editor opened, to notice changes from elsewhere
    original: String,
    /// the catalog when the editor opened, to find out what changed
    before: Vec<offer::CatalogItem>,
    pub offer: Offer,
    /// the template the project was made from, with its catalog
    pub template: Option<(PathBuf, Vec<offer::CatalogItem>)>,
    pub table: TableState,
    pub dirty: bool,
    /// VAT for the total, e.g. 0.19
    pub tax: f64,
}

impl OfferEditor {
    fn changed(&mut self) {
        self.dirty = true;
    }

    pub fn selected(&self) -> Option<usize> {
        self.table.selected().filter(|&i| i < self.offer.lines.len())
    }

    /// Products of the project catalog and the template that are not in the offer yet.
    pub fn addable(&self) -> Vec<offer::CatalogItem> {
        let template = self.template.iter().flat_map(|(_, catalog)| catalog);
        let mut items: Vec<offer::CatalogItem> = Vec::new();
        for item in self.offer.catalog.iter().chain(template) {
            let used = self.offer.lines.iter().any(|l| l.anchor.as_deref() == Some(&item.anchor));
            if !used && !items.iter().any(|i| i.anchor == item.anchor) {
                items.push(item.clone());
            }
        }
        items
    }

    /// Lines whose usage is not written down yet.
    pub fn uncounted(&self) -> usize {
        self.offer.lines.iter().filter(|l| !l.counted()).count()
    }

    /// Changes the number the mode is about by `delta`: the offered amount or the used one.
    fn step(&mut self, index: usize, delta: f64) {
        let mode = self.mode;
        let Some(line) = self.offer.lines.get_mut(index) else { return };
        match mode {
            EditorMode::Offer => {
                // returned items must have been offered
                let amount = line.amount + delta;
                if amount >= line.returned.unwrap_or(0.0) {
                    line.amount = amount;
                    self.changed();
                }
            },
            EditorMode::Usage => {
                let used = line.used() + delta;
                if used >= 0.0 {
                    line.set_used(used);
                    self.changed();
                }
            },
        }
    }
}

impl App {
    pub fn open_offer(&mut self, mode: EditorMode) {
        let Some(entry) = self.selected_entry() else { return };
        if let Location::Archive(_) = entry.location {
            self.notify(lformat!("archived projects are not changed, move it back first (a)"));
            return;
        }
        let file = entry.project.file();
        let name = entry.summary.name.clone();
        let original = match std::fs::read_to_string(&file) {
            Ok(content) => content,
            Err(error) => return self.fail(format!("{}: {}", super::paths::short(&file), error)),
        };
        let offer = match offer::read(&original) {
            Ok(offer) => offer,
            Err(error) => return self.fail(format!("{:#}", error)),
        };
        let tax = yaml_edit::value_at(&original, "tax")
            .and_then(|t| offer::parse_number(&t))
            .or_else(|| CONFIG.get_f64("defaults/tax"))
            .unwrap_or(0.19);
        let selected = if offer.lines.is_empty() { None } else { Some(0) };
        self.offer = Some(OfferEditor {
            mode,
            template: offer::template_catalog(&original),
            before: offer.catalog.clone(),
            file,
            name,
            original,
            offer,
            table: TableState::default().with_selected(selected),
            dirty: false,
            tax,
        });
        self.screen = Screen::Offer;
    }

    /// Back to the project page, without saving.
    pub fn close_offer(&mut self) {
        self.offer = None;
        if self.screen == Screen::Offer {
            self.screen = if self.detail.is_some() { Screen::Detail } else { self.previous };
        }
    }

    /// Esc in the editor: back, but changes are not lost without asking.
    pub fn leave_offer(&mut self) {
        let Some(editor) = self.offer.as_ref() else {
            self.close_offer();
            return;
        };
        if !editor.dirty {
            self.close_offer();
            return;
        }
        self.modal = Some(Modal::Dialog(Dialog::with(
            lformat!("Unsaved changes"),
            vec![lformat!("The changes to the offer of {} are not saved yet.", editor.name)],
            Tone::Warning,
            vec![
                Button::new(lformat!("Save"), Some('s'), Effect::OfferSave),
                Button::new(lformat!("Discard"), Some('d'), Effect::OfferDiscard),
                Button::new(lformat!("Keep editing"), Some('k'), Effect::Close),
            ],
            0,
            2,
        )));
    }

    /// `A` on the project page: notes in the file that the client accepted the offer, or takes it back.
    pub fn toggle_accepted(&mut self) {
        let Some(entry) = self.selected_entry() else { return };
        let file = entry.project.file();
        let name = entry.summary.name.clone();
        let accepted = self.is_accepted(&entry.project);
        let (title, question, yes) = if accepted {
            (
                lformat!("Offer accepted"),
                lformat!("Take back that the client accepted the offer for {}?", name),
                lformat!("Take back"),
            )
        } else {
            (
                lformat!("Offer accepted"),
                lformat!("Did the client accept the offer for {}?", name),
                lformat!("Yes, accepted"),
            )
        };
        self.modal = Some(Modal::Dialog(Dialog::confirm(
            title,
            vec![question],
            yes,
            Effect::Op(Op::SetAccepted { file, accepted: !accepted }),
            false,
        )));
    }

    pub fn editor_step(&mut self, delta: f64) {
        if let Some(editor) = self.offer.as_mut() {
            if let Some(i) = editor.selected() {
                editor.step(i, delta);
            }
        }
    }

    /// Usage: everything that was offered got used.
    pub fn editor_as_offered(&mut self) {
        if let Some(editor) = self.offer.as_mut() {
            if let Some(i) = editor.selected() {
                let line = &mut editor.offer.lines[i];
                line.set_used(line.amount);
                editor.changed();
            }
        }
    }

    /// Usage: nothing of it got used.
    pub fn editor_none_used(&mut self) {
        if let Some(editor) = self.offer.as_mut() {
            if let Some(i) = editor.selected() {
                editor.offer.lines[i].set_used(0.0);
                editor.changed();
            }
        }
    }

    pub fn editor_change_line(&mut self) {
        let selected = self.offer.as_ref().and_then(OfferEditor::selected);
        if selected.is_some() {
            self.offer_line_form(selected);
        }
    }

    pub fn editor_remove_line(&mut self) {
        let Some(editor) = self.offer.as_mut() else { return };
        let Some(i) = editor.selected() else { return };
        let usage = editor.mode == EditorMode::Usage;
        let (name, _, _) = editor.offer.resolved(&editor.offer.lines[i]);
        if usage && editor.offer.lines[i].amount > 0.0 {
            // what was offered stays in the offer, it just was not used
            editor.offer.lines[i].set_used(0.0);
            editor.changed();
            self.notify(lformat!("{}: nothing used (the offer keeps it)", name));
        } else {
            editor.offer.lines.remove(i);
            let left = editor.offer.lines.len();
            editor.table.select(if left == 0 { None } else { Some(i.min(left - 1)) });
            editor.changed();
            self.notify(lformat!("{} removed (not saved yet)", name));
        }
    }

    pub fn editor_add_product(&mut self) {
        let Some(editor) = self.offer.as_ref() else { return };
        let usage = editor.mode == EditorMode::Usage;
        let items = editor.addable();
        if items.is_empty() {
            self.notify(lformat!("every catalog product is in the offer already, n adds a new one"));
            return;
        }
        self.modal = Some(Modal::Pick(Pick::new(
            if usage { lformat!("Add a product that was used extra") } else { lformat!("Add a product") },
            items
                .iter()
                .map(|i| PickItem {
                    label: i.describe(),
                    disabled: None,
                })
                .collect(),
            0,
            PickPurpose::OfferAdd(items),
        )));
    }

    /// Adds a product picked from the catalog: offered once, or (usage) not offered and used once.
    pub fn add_picked(&mut self, item: &offer::CatalogItem) {
        let Some(editor) = self.offer.as_mut() else { return };
        let index = match editor.mode {
            EditorMode::Offer => editor.offer.add_item(item, 1.0),
            EditorMode::Usage => {
                let index = editor.offer.add_item(item, 0.0);
                editor.offer.lines[index].set_used(1.0);
                index
            },
        };
        editor.table.select(Some(index));
        editor.changed();
    }

    pub fn offer_line_form(&mut self, index: Option<usize>) {
        let Some(editor) = self.offer.as_ref() else { return };
        let usage = editor.mode == EditorMode::Usage;
        let comma = |n: f64| offer::number(n).replace('.', ",");
        let (name, unit, price, quantity, title, note) = match index.and_then(|i| editor.offer.lines.get(i)) {
            Some(line) => {
                let (name, unit, price) = editor.offer.resolved(line);
                let note = line
                    .anchor
                    .is_some()
                    .then(|| lformat!("Name, unit and price are part of the catalog of this project."));
                let quantity = if usage { line.used() } else { line.amount };
                (name, unit, comma(price), comma(quantity), lformat!("Change product"), note)
            },
            None => (String::new(), None, String::new(), String::from("1"), lformat!("New product"), None),
        };
        let quantity_field = match (usage, index.and_then(|i| editor.offer.lines.get(i))) {
            (true, Some(line)) => Field::new(
                "quantity",
                lformat!("Used"),
                FieldKind::Decimal,
                quantity,
                lformat!("actually used, offered were {}", comma(line.amount)),
            ),
            (true, None) => Field::new("quantity", lformat!("Used"), FieldKind::Decimal, quantity, lformat!("used, but not in the offer")),
            (false, _) => Field::new("quantity", lformat!("Quantity"), FieldKind::Decimal, quantity, lformat!("offered")),
        };
        let mut form = Form::new(
            title,
            FormPurpose::OfferLine { index },
            vec![
                Field::new("name", lformat!("Name"), FieldKind::Text, name, String::new()).required(),
                Field::new("unit", lformat!("Unit"), FieldKind::Text, unit.unwrap_or_default(), lformat!("e.g. 1l, stk, 400g; optional")),
                Field::new("price", lformat!("Price"), FieldKind::Decimal, price, lformat!("per unit, before tax, e.g. 2,50")).required(),
                quantity_field.required(),
            ],
        )
        .submit_label(lformat!("OK"))
        .focus_on(if index.is_some() { 3 } else { 0 });
        if let Some(note) = note {
            form = form.note(note);
        }
        self.modal = Some(Modal::Form(form));
    }

    /// Applies the product form to the editor. Errors name the field they belong to.
    pub fn submit_offer_line(&mut self, form: &Form, index: Option<usize>) -> Result<(), (String, String)> {
        let name = form.value("name");
        let unit = Some(form.value("unit")).filter(|u| !u.is_empty());
        let price = offer::parse_number(&form.value("price"))
            .filter(|p| *p >= 0.0)
            .ok_or_else(|| (String::from("price"), lformat!("please enter a price like 2,50")))?;
        let quantity = offer::parse_number(&form.value("quantity"))
            .filter(|a| *a >= 0.0)
            .ok_or_else(|| (String::from("quantity"), lformat!("please enter an amount like 4")))?;
        let Some(editor) = self.offer.as_mut() else { return Ok(()) };
        let usage = editor.mode == EditorMode::Usage;

        let index = match index {
            Some(i) => {
                let line = editor.offer.lines.get_mut(i).ok_or_else(|| (String::new(), String::new()))?;
                if usage {
                    line.set_used(quantity);
                } else {
                    if line.returned.is_some_and(|r| r > quantity) {
                        return Err((
                            String::from("quantity"),
                            lformat!(
                                "more was returned than that, the amount must be at least {}",
                                offer::number(line.returned.unwrap_or(0.0))
                            ),
                        ));
                    }
                    line.amount = quantity;
                }
                i
            },
            None => {
                // a product of the catalog or the template under the same name is reused
                let known = editor
                    .offer
                    .catalog
                    .iter()
                    .chain(editor.template.iter().flat_map(|(_, c)| c))
                    .find(|i| i.name.eq_ignore_ascii_case(&name) && i.unit == unit)
                    .cloned();
                // used extra: not offered
                let offered = if usage { 0.0 } else { quantity };
                let index = match known {
                    Some(item) => editor.offer.add_item(&item, offered),
                    None => editor.offer.add_new(name.clone(), unit.clone(), price, offered),
                };
                if usage {
                    editor.offer.lines[index].set_used(quantity);
                }
                index
            },
        };
        editor.offer.set_product(index, name, unit, price);
        editor.table.select(Some(index));
        editor.changed();
        Ok(())
    }

    pub fn save_offer(&mut self) {
        let Some(editor) = self.offer.as_mut() else { return };
        // saving the usage notes it for every product: what was not touched was used as offered
        if editor.mode == EditorMode::Usage && editor.uncounted() > 0 {
            for line in editor.offer.lines.iter_mut().filter(|l| !l.counted()) {
                line.set_used(line.amount);
            }
            editor.dirty = true;
        }
        if !editor.dirty {
            self.close_offer();
            self.notify(lformat!("nothing changed"));
            return;
        }
        match offer::write(&editor.original, &editor.offer) {
            Ok(content) => {
                let sync = editor
                    .template
                    .as_ref()
                    .map(|(path, catalog)| (path.clone(), offer::changes(&editor.before, &editor.offer.catalog, catalog)));
                self.pending = Some(Op::SaveOffer {
                    file: editor.file.clone(),
                    original: editor.original.clone(),
                    content,
                    sync,
                });
            },
            Err(error) => {
                self.quit_after_save = false;
                self.fail(format!("{:#}", error));
            },
        }
    }

    /// Asks which catalog changes go to the template; all are chosen at first, "Keep" leaves the template as is.
    pub fn ask_catalog_sync(&mut self, template: PathBuf, changes: Vec<offer::Change>) {
        self.show(Modal::CatalogSync {
            template,
            changes: changes.into_iter().map(|c| (c, true)).collect(),
            state: ratatui::widgets::ListState::default().with_selected(Some(0)),
            focus: 0,
        });
    }
}
