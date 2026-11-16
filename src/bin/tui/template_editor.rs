//! The Template tab: the catalog and the starting offer of a template, edited
//! like an offer and written with the same care (only the changed lines, read
//! back and checked before anything is saved).

use std::path::PathBuf;

use ratatui::widgets::TableState;

use asciii::{project::Project, CONFIG};

use super::{
	app::{App, Screen},
	dialogs::{Button, Dialog, Effect, Modal, Tone},
	forms::{Field, FieldKind, Form, FormPurpose},
	offer::{self, CatalogItem, Line, Offer},
	ops::Op,
	text_input::TextInput,
};

/// A row of the table: a catalog product, or a line of the starting offer that
/// has no catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
	/// index into `offer.catalog`
	Catalog(usize),
	/// index into `offer.lines`
	Loose(usize),
}

/// Where to go once the unsaved-changes question is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
	Stay,
	Screen(Screen),
	Project(usize),
	Quit,
}

pub struct TemplateEditor {
	/// every template in the templates folder
	pub names: Vec<String>,
	pub chosen: usize,
	/// the configured default (`template` in the configuration)
	pub default: String,
	pub file: Option<PathBuf>,
	/// the file as read, to notice changes from elsewhere
	original: String,
	saved: Offer,
	pub offer: Offer,
	pub has_products: bool,
	pub has_catalog: bool,
	pub table: TableState,
	pub search: TextInput,
	pub searching: bool,
	/// why nothing can be shown: no project folder, or the template cannot be
	/// read
	pub problem: Option<String>,
}

fn empty_offer() -> Offer {
	Offer {
		catalog: Vec::new(),
		lines: Vec::new(),
	}
}

impl TemplateEditor {
	/// Reads the templates; `wanted` is chosen if it exists, else the configured
	/// default.
	pub fn load(wanted: Option<&str>) -> Self {
		let default = CONFIG
			.get_str_or("template")
			.unwrap_or("default")
			.to_owned();
		let mut editor = TemplateEditor {
			names: Vec::new(),
			chosen: 0,
			default: default.clone(),
			file: None,
			original: String::new(),
			saved: empty_offer(),
			offer: empty_offer(),
			has_products: false,
			has_catalog: false,
			table: TableState::default(),
			search: TextInput::default(),
			searching: false,
			problem: None,
		};
		let storage = match asciii::storage::setup::<Project>() {
			Ok(storage) => storage,
			Err(error) => {
				editor.problem = Some(lformat!(
					"the project folder could not be opened: {}",
					error
				));
				return editor;
			},
		};
		let mut names = storage.list_template_names().unwrap_or_default();
		names.sort();
		editor.chosen = wanted
			.and_then(|w| names.iter().position(|n| n == w))
			.or_else(|| names.iter().position(|n| *n == default))
			.unwrap_or(0);
		editor.names = names;
		let Some(name) = editor.names.get(editor.chosen).cloned() else {
			return editor;
		};
		match storage.get_template_file(&name) {
			Ok(file) => editor.read(file),
			Err(error) => editor.problem = Some(format!("{:#}", error)),
		}
		editor
	}

	/// A single template file, as if it was the only one (for tests).
	#[cfg(test)]
	pub fn from_file(file: PathBuf) -> Self {
		let name = file
			.file_stem()
			.map(|s| s.to_string_lossy().into_owned())
			.unwrap_or_default();
		let mut editor = TemplateEditor {
			names: vec![name.clone()],
			chosen: 0,
			default: name,
			file: None,
			original: String::new(),
			saved: empty_offer(),
			offer: empty_offer(),
			has_products: false,
			has_catalog: false,
			table: TableState::default(),
			search: TextInput::default(),
			searching: false,
			problem: None,
		};
		editor.read(file);
		editor
	}

	/// Reads the same file again, e.g. after saving; the selection stays.
	pub fn reread(&mut self) {
		let Some(file) = self.file.clone() else {
			return;
		};
		let selected = self.table.selected();
		self.problem = None;
		self.saved = empty_offer();
		self.offer = empty_offer();
		self.read(file);
		let count = self.rows().len();
		if let Some(index) = selected.filter(|&i| i < count) {
			self.table.select(Some(index));
		}
	}

	fn read(
		&mut self,
		file: PathBuf,
	) {
		let content = match std::fs::read_to_string(&file) {
			Ok(content) => content,
			Err(error) => {
				self.problem = Some(format!("{}: {}", super::paths::short(&file), error));
				self.file = Some(file);
				return;
			},
		};
		match offer::read_template(&content) {
			Ok(read) => {
				self.has_products = offer::has_products(&content);
				self.has_catalog = offer::has_catalog(&content);
				self.saved = read.clone();
				self.offer = read;
			},
			Err(error) => self.problem = Some(lformat!("the template cannot be read: {:#}", error)),
		}
		self.original = content;
		self.file = Some(file);
		if !self.rows().is_empty() {
			self.table.select(Some(0));
		}
	}

	pub fn name(&self) -> String {
		self.names.get(self.chosen).cloned().unwrap_or_default()
	}

	pub fn dirty(&self) -> bool {
		self.offer != self.saved
	}

	/// The file changed on disk since it was read.
	pub fn changed_on_disk(&self) -> bool {
		self
			.file
			.as_ref()
			.is_some_and(|f| std::fs::read_to_string(f).map_or(true, |c| c != self.original))
	}

	/// Catalog products, then starting-offer lines without catalog entry;
	/// filtered by the search.
	pub fn rows(&self) -> Vec<Row> {
		let needle = self.search.text().trim().to_lowercase();
		let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);
		let catalog = (0..self.offer.catalog.len())
			.filter(|&i| matches(&self.offer.catalog[i].name))
			.map(Row::Catalog);
		let loose = (0..self.offer.lines.len())
			.filter(|&i| {
				self.offer.lines[i]
					.anchor
					.as_deref()
					.is_none_or(|a| self.offer.item(a).is_none())
			})
			.filter(|&i| matches(&self.offer.lines[i].name))
			.map(Row::Loose);
		catalog.chain(loose).collect()
	}

	pub fn selected(&self) -> Option<Row> {
		let rows = self.rows();
		self.table.selected().and_then(|i| rows.get(i).copied())
	}

	/// The starting-offer line of a row.
	pub fn line_of(
		&self,
		row: Row,
	) -> Option<usize> {
		match row {
			Row::Catalog(i) => {
				let anchor = &self.offer.catalog.get(i)?.anchor;
				self
					.offer
					.lines
					.iter()
					.position(|l| l.anchor.as_ref() == Some(anchor))
			},
			Row::Loose(i) => (i < self.offer.lines.len()).then_some(i),
		}
	}

	/// Name, unit, price of a row.
	pub fn product(
		&self,
		row: Row,
	) -> (String, Option<String>, f64) {
		match row {
			Row::Catalog(i) => {
				let item = &self.offer.catalog[i];
				(item.name.clone(), item.unit.clone(), item.price)
			},
			Row::Loose(i) => {
				let line = &self.offer.lines[i];
				(line.name.clone(), line.unit.clone(), line.price)
			},
		}
	}

	/// Products that differ from the file: added, removed, renamed, repriced,
	/// other starting quantity.
	pub fn changed_products(&self) -> usize {
		let key_of =
			|offer: &Offer, line: &Line| match line.anchor.as_deref().filter(|a| offer.item(a).is_some())
			{
				Some(anchor) => format!("&{}", anchor),
				None => format!("\"{}", line.name),
			};
		let state = |offer: &Offer, key: &str| -> (Option<CatalogItem>, Option<Line>) {
			let item = key.strip_prefix('&').and_then(|a| offer.item(a)).cloned();
			let line = offer
				.lines
				.iter()
				.find(|l| key_of(offer, l) == key)
				.cloned();
			(item, line)
		};
		let mut keys: Vec<String> = Vec::new();
		for offer in [&self.saved, &self.offer] {
			keys.extend(offer.catalog.iter().map(|i| format!("&{}", i.anchor)));
			keys.extend(offer.lines.iter().map(|l| key_of(offer, l)));
		}
		keys.sort();
		keys.dedup();
		keys
			.iter()
			.filter(|k| state(&self.saved, k) != state(&self.offer, k))
			.count()
	}

	/// Sets the starting quantity of a row; 0 takes it out of the starting offer.
	fn set_quantity(
		&mut self,
		row: Row,
		quantity: f64,
	) -> Result<(), String> {
		let line = self.line_of(row);
		if quantity > 0.0 && !self.has_products {
			return Err(lformat!("the template has no products section for a starting offer; add \"products:\" in the editor"));
		}
		match (line, row) {
			(Some(i), _) if quantity <= 0.0 => {
				self.offer.lines.remove(i);
			},
			(Some(i), _) => {
				let line = &mut self.offer.lines[i];
				if line.returned.is_some_and(|r| r > quantity) {
					return Err(lformat!(
						"it starts with {} returned, clear that first (u) or keep at least {}",
						offer::number(line.returned.unwrap_or(0.0)),
						offer::number(line.returned.unwrap_or(0.0))
					));
				}
				line.amount = quantity;
			},
			(None, Row::Catalog(i)) if quantity > 0.0 => {
				let item = self.offer.catalog[i].clone();
				self.offer.add_item(&item, quantity);
			},
			_ => {},
		}
		Ok(())
	}

	/// The table selection after rows changed.
	fn select(
		&mut self,
		row: Option<Row>,
	) {
		let rows = self.rows();
		let index = row.and_then(|r| rows.iter().position(|x| *x == r));
		let fallback = self
			.table
			.selected()
			.map(|i| i.min(rows.len().saturating_sub(1)));
		self.table.select(if rows.is_empty() {
			None
		} else {
			index.or(fallback).or(Some(0))
		});
	}
}

impl App {
	/// The Template tab: (re)reads the template unless it has unsaved changes.
	pub fn open_template(&mut self) {
		if self.template.as_ref().is_some_and(TemplateEditor::dirty) {
			return;
		}
		let wanted = self.template.as_ref().map(TemplateEditor::name);
		self.template = Some(TemplateEditor::load(wanted.as_deref()));
	}

	/// After an editor run or a reload: what is on disk now, if nothing is
	/// unsaved.
	pub fn refresh_template(&mut self) {
		if self
			.template
			.as_ref()
			.is_some_and(|t| !t.dirty() && t.changed_on_disk())
			|| self.template.is_none()
		{
			if self.screen == Screen::Template {
				let wanted = self.template.as_ref().map(TemplateEditor::name);
				self.template = Some(TemplateEditor::load(wanted.as_deref()));
			}
		}
	}

	/// ←/→: another template.
	pub fn choose_template(
		&mut self,
		delta: isize,
	) {
		let Some(editor) = self.template.as_ref() else {
			return;
		};
		let count = editor.names.len();
		if count < 2 {
			return;
		}
		let next = (editor.chosen as isize + delta).rem_euclid(count as isize) as usize;
		let name = editor.names[next].clone();
		self.template = Some(TemplateEditor::load(Some(&name)));
	}

	/// Whether leaving the tab (to `after`) has to ask first; asks if so.
	pub fn template_blocks(
		&mut self,
		after: After,
	) -> bool {
		let Some(editor) = self
			.template
			.as_ref()
			.filter(|t| t.dirty() && self.screen == Screen::Template)
		else {
			return false;
		};
		let name = editor.name();
		let quit = after == After::Quit;
		self.modal = Some(Modal::Dialog(Dialog::with(
			lformat!("Unsaved changes"),
			vec![lformat!(
				"The changes to the template {} are not saved yet.",
				name
			)],
			Tone::Warning,
			vec![
				Button::new(
					if quit {
						lformat!("Save and quit")
					} else {
						lformat!("Save")
					},
					Some('s'),
					Effect::TemplateSave(after),
				),
				Button::new(
					if quit {
						lformat!("Quit without saving")
					} else {
						lformat!("Discard")
					},
					Some('d'),
					Effect::TemplateDiscard(after),
				),
				Button::new(lformat!("Keep editing"), Some('k'), Effect::Close),
			],
			0,
			2,
		)));
		true
	}

	/// Goes on after the unsaved-changes question.
	pub fn template_continue(
		&mut self,
		after: After,
	) {
		match after {
			After::Stay => {},
			After::Screen(screen) => self.go(screen),
			After::Project(index) => self.open_project(index, None),
			After::Quit => self.quit = true,
		}
	}

	pub fn discard_template(
		&mut self,
		after: After,
	) {
		match self.template.as_mut() {
			Some(editor) if after == After::Stay => editor.reread(),
			_ => self.template = None,
		}
		self.notify(lformat!("changes discarded"));
		self.template_continue(after);
	}

	/// `s`: writes the template, only if the file did not change meanwhile and
	/// the result checks out.
	pub fn save_template(
		&mut self,
		after: After,
	) {
		let Some(editor) = self.template.as_ref() else {
			return;
		};
		if !editor.dirty() {
			self.notify(lformat!("nothing changed"));
			self.template_continue(after);
			return;
		}
		let Some(file) = editor.file.clone() else {
			return;
		};
		if editor.changed_on_disk() {
			self.fail(lformat!(
				"{} was changed by someone else while editing, nothing saved; Esc → Discard reads it again",
				super::paths::short(&file)
			));
			return;
		}
		let content = match offer::write_template(&editor.original, &editor.offer) {
			Ok(content) => content,
			Err(error) => return self.fail(format!("{:#}", error)),
		};
		if let Err(error) = std::fs::write(&file, &content) {
			return self.fail(format!("{}: {}", super::paths::short(&file), error));
		}
		// read back: what is on disk now is what was checked
		if std::fs::read_to_string(&file).ok().as_deref() != Some(content.as_str()) {
			return self.fail(lformat!(
				"{} could not be read back after saving",
				super::paths::short(&file)
			));
		}
		let changed = editor.changed_products();
		let name = editor.name();
		if let Some(editor) = self.template.as_mut() {
			editor.reread();
		}
		self.success(if changed == 1 {
			lformat!("template {} saved: 1 product changed", name)
		} else {
			lformat!("template {} saved: {} products changed", name, changed)
		});
		self.template_continue(after);
	}

	/// `Shift+E`: the template file in the editor.
	pub fn edit_template_file(&mut self) {
		if let Some(file) = self.template.as_ref().and_then(|t| t.file.clone()) {
			self.pending = Some(Op::Edit(file));
		}
	}

	/// `+`/`-`: the starting quantity of the highlighted product.
	pub fn template_step(
		&mut self,
		delta: f64,
	) {
		let Some(editor) = self.template.as_mut() else {
			return;
		};
		let Some(row) = editor.selected() else { return };
		let current = editor
			.line_of(row)
			.map_or(0.0, |i| editor.offer.lines[i].amount);
		let wanted = (current + delta).max(0.0);
		if wanted == current {
			return;
		}
		let (name, _, _) = editor.product(row);
		match editor.set_quantity(row, wanted) {
			Ok(()) => {
				editor.select(Some(row));
				if wanted == 0.0 {
					self.notify(lformat!("{} is no longer in the starting offer", name));
				}
			},
			Err(error) => self.fail(error),
		}
	}

	/// `u`: the starting offer keeps no sold or returned numbers of the
	/// highlighted product.
	pub fn template_clear_usage(&mut self) {
		let Some(editor) = self.template.as_mut() else {
			return;
		};
		let Some(i) = editor.selected().and_then(|r| editor.line_of(r)) else {
			return;
		};
		let line = &mut editor.offer.lines[i];
		line.sold = None;
		line.returned = None;
		self.notify(lformat!("usage cleared (not saved yet)"));
	}

	/// `d`: asks before a product goes.
	pub fn template_ask_remove(&mut self) {
		let Some(editor) = self.template.as_ref() else {
			return;
		};
		let Some(row) = editor.selected() else { return };
		let (name, _, _) = editor.product(row);
		let mut lines = vec![match row {
			Row::Catalog(_) => lformat!("Remove {} from the catalog of the template?", name),
			Row::Loose(_) => lformat!("Remove {} from the starting offer?", name),
		}];
		if matches!(row, Row::Catalog(_)) && editor.line_of(row).is_some() {
			lines.push(lformat!(
				"It is in the starting offer too and is removed there as well."
			));
		}
		lines.push(lformat!("Existing projects keep their own catalog."));
		self.modal = Some(Modal::Dialog(Dialog::confirm(
			lformat!("Remove product"),
			lines,
			lformat!("Remove"),
			Effect::TemplateRemove(row),
			false,
		)));
	}

	pub fn template_remove(
		&mut self,
		row: Row,
	) {
		let Some(editor) = self.template.as_mut() else {
			return;
		};
		let (name, _, _) = editor.product(row);
		let line = editor.line_of(row);
		if let Some(i) = line {
			editor.offer.lines.remove(i);
		}
		if let Row::Catalog(i) = row {
			editor.offer.catalog.remove(i);
		}
		editor.select(None);
		self.notify(match (row, line) {
			(Row::Catalog(_), Some(_)) => lformat!(
				"{} removed, also from the starting offer (not saved yet)",
				name
			),
			_ => lformat!("{} removed (not saved yet)", name),
		});
	}

	/// `n` (`None`) or Enter/`e` on a row: name, unit, price and starting
	/// quantity.
	pub fn template_product_form(
		&mut self,
		row: Option<Row>,
	) {
		let Some(editor) = self.template.as_ref() else {
			return;
		};
		let comma = |n: f64| offer::number(n).replace('.', ",");
		let (name, unit, price, quantity, title) = match row {
			Some(row) => {
				let (name, unit, price) = editor.product(row);
				let quantity = editor
					.line_of(row)
					.map(|i| comma(editor.offer.lines[i].amount))
					.unwrap_or_default();
				(
					name,
					unit.unwrap_or_default(),
					comma(price),
					quantity,
					lformat!("Change product"),
				)
			},
			None => (
				String::new(),
				String::new(),
				String::new(),
				String::new(),
				lformat!("New product"),
			),
		};
		let quantity_hint = if editor.has_products {
			lformat!("how many a new project offers; empty or 0: not in the starting offer")
		} else {
			lformat!("the template has no products section, so no starting offer")
		};
		let form = Form::new(
			title,
			FormPurpose::TemplateProduct { row },
			vec![
				Field::new(
					"name",
					lformat!("Name"),
					FieldKind::Text,
					name,
					String::new(),
				)
				.required(),
				Field::new(
					"unit",
					lformat!("Unit"),
					FieldKind::Text,
					unit,
					lformat!("e.g. 1l, stk, 400g; optional"),
				),
				Field::new(
					"price",
					lformat!("Price"),
					FieldKind::Decimal,
					price,
					lformat!("per unit, before tax, e.g. 2,50"),
				)
				.required(),
				Field::new(
					"quantity",
					lformat!("Starting offer"),
					FieldKind::Decimal,
					quantity,
					quantity_hint,
				),
			],
		)
		.submit_label(lformat!("OK"))
		.note(lformat!(
			"New projects start with this. Existing projects keep their own catalog."
		));
		self.modal = Some(Modal::Form(form));
	}

	/// Applies the product form. Errors name their field.
	pub fn submit_template_product(
		&mut self,
		form: &Form,
		row: Option<Row>,
	) -> Result<(), (String, String)> {
		let name = form.value("name");
		let unit = Some(form.value("unit")).filter(|u| !u.is_empty());
		let price = offer::parse_number(&form.value("price"))
			.filter(|p| *p >= 0.0)
			.ok_or_else(|| {
				(
					String::from("price"),
					lformat!("please enter a price like 2,50"),
				)
			})?;
		let quantity_text = form.value("quantity");
		let quantity = if quantity_text.is_empty() {
			0.0
		} else {
			offer::parse_number(&quantity_text)
				.filter(|a| *a >= 0.0)
				.ok_or_else(|| {
					(
						String::from("quantity"),
						lformat!("please enter an amount like 4"),
					)
				})?
		};
		let Some(editor) = self.template.as_mut() else {
			return Ok(());
		};
		if quantity > 0.0 && !editor.has_products {
			return Err((
                String::from("quantity"),
                lformat!("the template has no products section for a starting offer; add \"products:\" in the editor"),
            ));
		}
		// a new name that the catalog has already would be confusing in the offer
		// editor (names that are there twice already, e.g. two sizes, stay possible)
		let renamed = row.is_none_or(|r| {
			let (old, old_unit, _) = editor.product(r);
			!old.eq_ignore_ascii_case(&name) || old_unit != unit
		});
		let other = |i: usize| row != Some(Row::Catalog(i));
		if renamed
			&& editor
				.offer
				.catalog
				.iter()
				.enumerate()
				.any(|(i, item)| other(i) && item.name.eq_ignore_ascii_case(&name) && item.unit == unit)
		{
			return Err((
				String::from("name"),
				lformat!("the catalog has {} already", name),
			));
		}
		let before = editor.clone_offer();
		let row = match row {
			Some(Row::Catalog(i)) => {
				// the anchor stays: projects made from the template refer to it
				let item = &mut editor.offer.catalog[i];
				item.name = name;
				item.unit = unit;
				item.price = price;
				Row::Catalog(i)
			},
			Some(Row::Loose(i)) => {
				let line = &mut editor.offer.lines[i];
				line.name = name;
				line.unit = unit;
				line.price = price;
				Row::Loose(i)
			},
			None => {
				let anchor = editor.offer.free_anchor(&name);
				editor.offer.catalog.push(CatalogItem {
					anchor,
					name,
					price,
					unit,
					tax: None,
				});
				Row::Catalog(editor.offer.catalog.len() - 1)
			},
		};
		if let Err(error) = editor.set_quantity(row, quantity) {
			editor.offer = before;
			return Err((String::from("quantity"), error));
		}
		// a new product shows even when the search would hide it
		if !editor.rows().contains(&row) {
			editor.search.clear();
			editor.searching = false;
		}
		// a loose line set to 0 is gone
		let row = if matches!(row, Row::Loose(_)) && quantity == 0.0 {
			None
		} else {
			Some(row)
		};
		editor.select(row);
		Ok(())
	}

	/// `/` in the template tab.
	pub fn template_search(&mut self) {
		if let Some(editor) = self.template.as_mut() {
			editor.searching = true;
		}
	}
}

impl TemplateEditor {
	fn clone_offer(&self) -> Offer {
		self.offer.clone()
	}
}
