//! What the project actions do: open forms and dialogs, or hand an [`Op`] to
//! the main loop.

use std::path::{Path, PathBuf};

use chrono::prelude::*;

use asciii::{
	project::{BillType, Project},
	stats,
	storage::Storable,
	CONFIG,
};

use super::{
	actions::Action,
	app::{App, Screen},
	data::{Entry, Location},
	dialogs::{Button, Dialog, Effect, Modal, Pick, PickItem, PickPurpose, Tone, Typed},
	forms::{self, Field, FieldKind, Form, FormPurpose},
	ops::{GitOp, Op},
	paths,
	text_input::TextInput,
	yaml_edit,
};

impl App {
	/// `p`: one form with every field of the project file, empty ones marked and
	/// focused first.
	pub fn open_details_form(&mut self) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let file = entry.project.file();
		let name = entry.summary.name.clone();
		let content = std::fs::read_to_string(&file).unwrap_or_default();
		let fields = forms::project_fields(&content);
		if fields.is_empty() {
			self.notify(lformat!(
				"{} has none of the fields this form fills in; use Edit file",
				name
			));
			return;
		}
		let empty = fields.iter().filter(|f| f.missing).count();
		let note = match empty {
			0 => {
				lformat!("Everything is filled in. Change what you need; only changed fields are written.")
			},
			n => lformat!(
				"{} fields are still empty (marked !). Only changed fields are written.",
				n
			),
		};
		self.modal = Some(Modal::Form(
			Form::new(
				lformat!("Details of {}", name),
				FormPurpose::Fill { file },
				fields,
			)
			.note(note),
		));
	}

	/// One field, for the guided next step: dates start with today.
	pub fn fill_one(
		&mut self,
		path: &'static str,
	) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let file = entry.project.file();
		let name = entry.summary.name.clone();
		let content = std::fs::read_to_string(&file).unwrap_or_default();
		let Some(mut field) = forms::project_fields(&content)
			.into_iter()
			.find(|f| f.key == path)
		else {
			self.notify(lformat!(
				"\"{}\" is not in the project file; use Edit file",
				yaml_edit::label(path)
			));
			return;
		};
		let mut note = None;
		if field.kind == FieldKind::Date && field.missing {
			field.input.set(self.today.format("%d.%m.%Y").to_string());
			note = Some(lformat!("Today's date is filled in; change it if needed."));
		}
		let mut form = Form::new(
			format!("{} · {}", field.label, name),
			FormPurpose::Fill { file },
			vec![field],
		);
		form.set_focus(0);
		if let Some(note) = note {
			form = form.note(note);
		}
		self.modal = Some(Modal::Form(form));
	}

	/// `I`: the next free invoice number of the year, after a confirmation.
	pub fn assign_invoice_number(&mut self) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let project = &entry.project;
		let file = project.file();
		let name = project.short_desc();
		// the sequence belongs to the year the invoice is written in
		let year = stats::invoice_year(project).unwrap_or_else(|| self.today.year());
		let number = self
			.data
			.as_ref()
			.map(|d| {
				let all: Vec<&Project> = d.entries.iter().map(|e| &e.project).collect();
				stats::next_invoice_number(&all, year)
			})
			.unwrap_or(1);
		self.modal = Some(Modal::Dialog(Dialog::confirm(
			lformat!("Invoice number"),
			vec![
				lformat!("Assign invoice number {} ({}) to {}?", number, year, name),
				lformat!(
					"It is the next free number of {}. Once invoices are sent, numbers must not change.",
					year
				),
			],
			lformat!("Assign {}", number),
			Effect::Op(Op::SetInvoiceNumber { file, number }),
			false,
		)));
	}

	/// `m`: which document to create.
	pub fn open_make_menu(&mut self) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let project = &entry.project;
		let file = project.file();
		let name = project.short_desc();
		let invoice = Action::MakeInvoice.availability(self);
		let disabled = match invoice {
			super::actions::Avail::Disabled(reason) => Some(reason),
			_ => None,
		};
		let ready = project.is_missing_for_invoice().is_empty() && disabled.is_none();
		self.modal = Some(Modal::Pick(Pick::new(
			lformat!("Create a PDF for {}", name),
			vec![
				PickItem {
					label: lformat!("Offer"),
					disabled: None,
				},
				PickItem {
					label: lformat!("Invoice"),
					disabled,
				},
			],
			if ready { 1 } else { 0 },
			PickPurpose::Make(file),
		)));
	}

	pub fn make(
		&mut self,
		bill_type: BillType,
	) {
		let Some(file) = self.selected_entry().map(|e| e.project.file()) else {
			return;
		};
		let next = self.request_make(file, bill_type, false);
		self.keep_modal(next);
	}

	/// Starts creating a document, after the checks that may need the user's say:
	/// duplicate numbers (invoices only, skipped once the user chose to ignore
	/// them) and an up-to-date PDF.
	pub fn request_make(
		&mut self,
		file: PathBuf,
		bill_type: BillType,
		duplicates_ignored: bool,
	) -> Option<Modal> {
		if matches!(bill_type, BillType::Invoice) && !duplicates_ignored {
			let lines = self.duplicate_numbers(&file);
			if !lines.is_empty() {
				let mut text = vec![lformat!("This invoice shares numbers with other projects:")];
				text.extend(lines.iter().map(|l| format!("  • {}", l)));
				text.push(String::new());
				text.push(lformat!(
					"Invoice numbers must be unique per year. Fix the number in Project details first."
				));
				return Some(Modal::Dialog(Dialog::with(
					lformat!("Duplicate number"),
					text,
					Tone::Warning,
					vec![
						Button::new(lformat!("Back"), Some('b'), Effect::Close),
						Button::new(
							lformat!("Project details"),
							Some('p'),
							Effect::Action(Action::Details),
						),
						Button::new(
							lformat!("Create anyway"),
							Some('i'),
							Effect::MakeIgnoringDuplicates { file, bill_type },
						),
					],
					0,
					0,
				)));
			}
		}

		let up_to_date = self
			.data
			.as_ref()
			.and_then(|d| d.find_by_file(&file).map(|i| &d.entries[i].project))
			.and_then(|project| pdf_up_to_date(project, bill_type));
		match up_to_date {
			// the library would skip it, so ask instead of silently doing nothing
			Some(pdf) => Some(Modal::Dialog(Dialog::with(
				lformat!("PDF is up to date"),
				vec![lformat!(
					"{} is up to date. Create it again anyway?",
					paths::short(&pdf)
				)],
				Tone::Info,
				vec![
					Button::new(
						lformat!("Create again"),
						Some('y'),
						Effect::Op(Op::Make {
							file,
							bill_type: Some(bill_type),
							force: true,
						}),
					),
					Button::new(lformat!("Open it"), Some('o'), Effect::OpenPath(pdf)),
					Button::new(lformat!("Cancel"), Some('n'), Effect::Close),
				],
				0,
				2,
			))),
			None => {
				self.pending = Some(Op::Make {
					file,
					bill_type: Some(bill_type),
					force: false,
				});
				None
			},
		}
	}

	/// One line per number of `file` that another project uses too: invoice
	/// numbers against all projects of the same year (archived ones included,
	/// they must be unique), offer numbers against the working projects.
	fn duplicate_numbers(
		&self,
		file: &Path,
	) -> Vec<String> {
		let Some(data) = self.data.as_ref() else {
			return Vec::new();
		};
		let Some(index) = data.find_by_file(file) else {
			return Vec::new();
		};
		let project = &data.entries[index].project;

		let mut lines = Vec::new();
		for (kind, working_only) in [
			(stats::NumberKind::Invoice, false),
			(stats::NumberKind::Offer, true),
		] {
			let entries: Vec<&Entry> = data
				.entries
				.iter()
				.filter(|e| !working_only || e.location == Location::Working)
				.collect();
			let projects: Vec<&Project> = entries.iter().map(|e| &e.project).collect();
			for conflict in stats::number_conflicts(project, &projects) {
				if conflict.kind != kind {
					continue;
				}
				let other = entries[conflict.other];
				let other = match other.location {
					Location::Working => other.summary.name.clone(),
					Location::Archive(year) => lformat!("{} (archive {})", other.summary.name, year),
				};
				lines.push(match kind {
					stats::NumberKind::Invoice => {
						lformat!(
							"Invoice number {} is also used by \"{}\"",
							conflict.number,
							other
						)
					},
					stats::NumberKind::Offer => lformat!(
						"Offer number {} is also used by \"{}\"",
						conflict.number,
						other
					),
				});
			}
		}
		lines
	}

	/// Why `value` cannot be the invoice number of `file`: not a number, or
	/// already used in that year.
	pub fn invoice_number_problem(
		&self,
		file: &Path,
		value: &str,
	) -> Option<String> {
		let Ok(number) = value.parse::<u32>() else {
			return Some(lformat!(
				"the invoice number must be a whole number like 42"
			));
		};
		let data = self.data.as_ref()?;
		let project = &data.entries[data.find_by_file(file)?].project;
		let year = stats::invoice_year(project).unwrap_or_else(|| self.today.year());
		let all: Vec<&Project> = data.entries.iter().map(|e| &e.project).collect();
		let user = stats::invoice_number_user(&all, year, number, Some(project))?;
		Some(lformat!(
			"invoice number {} is already used in {} by \"{}\", the next free one is {}",
			number,
			year,
			data.entries[user].summary.name,
			stats::next_invoice_number(&all, year)
		))
	}

	/// `e`: the project file (or the file of a broken project) in the editor.
	pub fn edit_file(&mut self) {
		let file = match self.selected_broken() {
			Some(broken) => project_file_in(&broken.dir),
			None => self.selected_entry().map(|e| e.project.file()),
		};
		match file {
			Some(file) => self.pending = Some(Op::Edit(file)),
			None => self.notify(lformat!("no project file found in this folder")),
		}
	}

	pub fn open_folder(&mut self) {
		if let Some(dir) = self.selected_entry().map(|e| e.project.dir()) {
			self.apply(Effect::OpenPath(dir));
		}
	}

	/// `a` on the project page: archive (ready or not, with a warning) or move
	/// back from the archive.
	pub fn archive(&mut self) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let project = &entry.project;
		let dir = project.dir();
		let name = project.short_desc();
		if let Location::Archive(year) = entry.location {
			self.modal = Some(Modal::Dialog(Dialog::with(
                lformat!("Move back from the archive"),
                vec![
                    lformat!("{} is archived in {}.", name, year),
                    lformat!("Moving it back puts it into the working directory again: it counts as a running project, shows up in the to-dos and can be changed."),
                ],
                Tone::Warning,
                vec![
                    Button::new(lformat!("Cancel"), Some('n'), Effect::Close),
                    Button::new(lformat!("Move back"), Some('y'), Effect::Op(Op::Unarchive { dir })),
                ],
                0,
                0,
            )));
			return;
		}
		let missing = Project::is_ready_for_archive(project);
		let year = project
			.year()
			.map(|y| y.to_string())
			.unwrap_or_else(|| self.today.year().to_string());
		if missing.is_empty() {
			let note = lformat!(
				"{} is ready to be archived. It moves into the archive folder of this year:",
				name
			);
			self.archive_form(dir, name, year, note);
		} else {
			self.modal = Some(Modal::Dialog(Dialog::with(
                lformat!("Not ready for the archive"),
                vec![
                    lformat!("{} is missing:", name),
                    format!("  • {}", yaml_edit::labels(&missing)),
                    String::new(),
                    lformat!("Archived projects are not changed any more. Fill these in first (Project details), or archive anyway."),
                ],
                Tone::Warning,
                vec![
                    Button::new(lformat!("Cancel"), Some('n'), Effect::Close),
                    Button::new(lformat!("Project details"), Some('p'), Effect::Action(Action::Details)),
                    Button::new(
                        lformat!("Archive anyway"),
                        Some('y'),
                        Effect::ArchiveForm {
                            dir,
                            name: name.clone(),
                            year,
                            note: lformat!("{} is NOT ready, it is archived anyway into the folder of this year:", name),
                        },
                    ),
                ],
                0,
                0,
            )));
		}
	}

	pub fn archive_form(
		&mut self,
		dir: PathBuf,
		name: String,
		year: String,
		note: String,
	) {
		self.modal = Some(Modal::Form(
			Form::new(
				lformat!("Archive {}", name),
				FormPurpose::Archive { dir },
				vec![Field::new(
					"year",
					lformat!("Year"),
					FieldKind::Number,
					year,
					lformat!("the archive folder"),
				)
				.required()],
			)
			.note(note)
			.submit_label(lformat!("Archive")),
		));
	}

	/// `D`: the name has to be typed.
	pub fn delete(&mut self) {
		let Some(entry) = self.selected_entry() else {
			return;
		};
		let dir = entry.project.dir();
		let name = entry.project.short_desc();
		self.modal = Some(Modal::Typed(Typed {
			message: lformat!(
				"Delete {} and its folder {}? This cannot be undone.",
				name,
				paths::short(&dir)
			),
			expected: name,
			input: TextInput::default(),
			op: Op::Delete { dir },
			error: None,
			focus: 0,
		}));
	}

	pub fn open_new_project_form(&mut self) {
		let templates = self
			.data
			.as_ref()
			.map(|d| d.templates.clone())
			.unwrap_or_default();
		let default_template = CONFIG
			.get_str_or("template")
			.unwrap_or("default")
			.to_owned();
		let selected = templates
			.iter()
			.position(|t| *t == default_template)
			.unwrap_or(0);
		let manager = CONFIG.get_str_or("user/name").unwrap_or("").to_owned();
		let mut edit = Field::toggle(
			"edit",
			lformat!("Open in editor"),
			false,
			lformat!("afterwards, open the new file in your editor"),
		);
		if !self.tools.editor {
			edit.locked = Some(lformat!(
				"no editor found: set $EDITOR, e.g. to \"code --wait\""
			));
			edit.hint = lformat!("no editor found ($EDITOR)");
		}
		let form = Form::new(
			lformat!("New project"),
			FormPurpose::NewProject,
			vec![
				Field::new(
					"name",
					lformat!("Name"),
					FieldKind::Text,
					"",
					lformat!("e.g. Sommerball"),
				)
				.required(),
				Field::choice(
					"template",
					lformat!("Template"),
					templates,
					selected,
					lformat!("←/→ or Space to choose"),
				),
				Field::new(
					"date",
					lformat!("Date"),
					FieldKind::Date,
					"",
					lformat!("e.g. 24.12.2026; empty = in two weeks"),
				),
				Field::new(
					"start",
					lformat!("Start time"),
					FieldKind::Text,
					"",
					lformat!("e.g. 18:00; optional"),
				),
				Field::new(
					"end",
					lformat!("End time"),
					FieldKind::Text,
					"",
					lformat!("e.g. 23:00; optional"),
				),
				Field::new(
					"manager",
					lformat!("Manager"),
					FieldKind::Text,
					manager,
					String::new(),
				),
				Field::new(
					"description",
					lformat!("Description"),
					FieldKind::Text,
					"",
					lformat!("optional"),
				),
				edit,
			],
		)
		.note(lformat!(
			"The project page opens afterwards and shows what to do next."
		))
		.submit_label(lformat!("Create"));
		self.modal = Some(Modal::Form(form));
	}

	/// `g`: the git commands.
	pub fn open_git_menu(&mut self) {
		self.modal = Some(Modal::Pick(Pick::new(
			lformat!("git"),
			GitOp::ALL
				.iter()
				.map(|op| PickItem {
					label: op.label(),
					disabled: None,
				})
				.collect(),
			0,
			PickPurpose::Git,
		)));
	}

	/// Runs `op`; pull and push ask first.
	pub fn git(
		&mut self,
		op: GitOp,
	) {
		if matches!(op, GitOp::Push | GitOp::Pull) {
			let (title, yes) = if op == GitOp::Push {
				(lformat!("Push to the remote?"), lformat!("Push"))
			} else {
				(lformat!("Pull from the remote?"), lformat!("Pull"))
			};
			self.modal = Some(Modal::Dialog(Dialog::confirm(
				lformat!("git"),
				vec![title],
				yes,
				Effect::Op(Op::Git(op)),
				false,
			)));
		} else {
			self.pending = Some(Op::Git(op));
		}
	}

	/// `x` / `c`: names the file first, then writes it.
	pub fn ask_export(
		&mut self,
		ics: bool,
	) {
		let year = self.year;
		let path = super::ops::export_path(year, ics);
		let (title, op) = if ics {
			(lformat!("Export calendar"), Op::ExportIcs { year })
		} else {
			(lformat!("Export CSV"), Op::ExportCsv { year })
		};
		let exists = path.exists();
		let mut lines = vec![lformat!(
			"Write all projects of {} to {}?",
			year,
			paths::short(&path)
		)];
		if exists {
			lines.push(lformat!("The file exists and is replaced."));
		}
		self.modal = Some(Modal::Dialog(Dialog::confirm(
			title,
			lines,
			lformat!("Export"),
			Effect::Op(op),
			exists,
		)));
	}

	/// The start page: creating the folders of an empty project storage.
	pub fn ask_create_storage(&mut self) {
		let no_template = self.screen == super::app::Screen::Template
			&& self.template.as_ref().is_some_and(|t| t.names.is_empty());
		let Some(root) = self
			.storage_missing()
			.or_else(|| no_template.then(asciii::storage::get_storage_path))
		else {
			return;
		};
		self.modal = Some(Modal::Dialog(Dialog::confirm(
			lformat!("Create empty project folder"),
			vec![
				lformat!(
					"Create {} with the folders working, archive and templates?",
					paths::tilde(&root)
				),
				lformat!("A default template is put into templates. Existing files are not touched."),
			],
			lformat!("Create"),
			Effect::CreateStorage,
			false,
		)));
	}

	pub fn create_storage(&mut self) {
		match super::ops::create_storage() {
			Ok(message) => {
				self.success(message);
				self.start_loading(None);
				if self.screen == super::app::Screen::Template {
					self.template = None;
					self.open_template();
				}
			},
			Err(error) => self.fail(format!("{:#}", error)),
		}
	}

	/// Enter in the broken scope: the whole reason and the file.
	pub fn show_broken(&mut self) {
		let Some(broken) = self.selected_broken() else {
			return;
		};
		let name = broken
			.dir
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_default();
		let file = project_file_in(&broken.dir);
		let mut lines = vec![
			lformat!(
				"The folder {} cannot be read as a project:",
				paths::short(&broken.dir)
			),
			format!("  {}", broken.error),
			String::new(),
		];
		match &file {
			Some(file) => lines.push(lformat!("File: {}", paths::short(file))),
			None => lines.push(lformat!("There is no .yml project file in the folder.")),
		}
		if file.is_some() {
			lines.push(lformat!(
				"The Log tab has the details of the YAML error (line and column)."
			));
		}
		let mut buttons = Vec::new();
		if file.is_some() {
			buttons.push(Button::new(
				lformat!("Edit file"),
				Some('e'),
				Effect::Action(Action::EditFile),
			));
			buttons.push(Button::new(
				lformat!("Open log"),
				Some('l'),
				Effect::Action(Action::Log),
			));
		}
		buttons.push(Button::new(lformat!("Close"), Some('c'), Effect::Close));
		let close = buttons.len() - 1;
		let focus = if file.is_some() && self.tools.editor {
			0
		} else {
			close
		};
		self.modal = Some(Modal::Dialog(Dialog::with(
			lformat!("Broken: {}", name),
			lines,
			Tone::Warning,
			buttons,
			focus,
			close,
		)));
	}

	/// The buttons of the start page, with their actions.
	pub fn first_run_buttons(&self) -> Vec<(String, Action)> {
		let mut buttons = vec![(lformat!("Try again"), Action::Reload)];
		if self.storage_missing().is_some() {
			buttons.push((
				lformat!("Create empty project folder"),
				Action::CreateStorage,
			));
		}
		buttons.push((lformat!("Open log"), Action::Log));
		buttons.push((lformat!("Quit"), Action::Quit));
		buttons
	}

	pub fn press_first_run(
		&mut self,
		index: usize,
	) {
		if self.data.is_some() || self.screen == Screen::Log {
			return;
		}
		if let Some((_, action)) = self.first_run_buttons().get(index) {
			self.first_run_focus = index;
			self.run(*action);
		}
	}
}

/// The `.yml` file of a project folder.
pub fn project_file_in(dir: &Path) -> Option<PathBuf> {
	let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
		.ok()?
		.filter_map(Result::ok)
		.map(|e| e.path())
		.filter(|p| p.extension().is_some_and(|e| e == "yml"))
		.collect();
	files.sort();
	files.into_iter().next()
}

/// The PDF of `bill_type`, if it exists and is up to date: the same rule
/// `document_export` uses to skip a build (its `.tex` file is newer than the
/// project file).
fn pdf_up_to_date(
	project: &Project,
	bill_type: BillType,
) -> Option<PathBuf> {
	use asciii::project::Exportable;
	use std::fs;

	let pdf = project.output_file(bill_type).filter(|pdf| pdf.exists())?;
	let tex_ext = CONFIG.get_str("extensions/output_file");
	let tex_name = match bill_type {
		BillType::Offer => project.offer_file_name(tex_ext),
		BillType::Invoice => project.invoice_file_name(tex_ext),
	}?;
	let modified = |path: &Path| fs::metadata(path).and_then(|m| m.modified()).ok();
	let tex = modified(&project.dir().join(tex_name))?;
	let file = modified(&project.file())?;
	(tex > file).then_some(pdf)
}
