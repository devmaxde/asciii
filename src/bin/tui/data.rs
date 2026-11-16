//! Loading all projects in a background thread.

use std::{
	path::{Path, PathBuf},
	sync::mpsc::{self, Receiver},
	thread,
	time::{Duration, Instant},
};

use anyhow::Error;
use chrono::prelude::*;

use asciii::{
	project::{
		spec::{HasEmployees, Invoicable, IsProject, Redeemable},
		Exportable, Project,
	},
	storage::{self, repo::GitStatus, Storable, Storage, StorageDir},
	util::currency_to_string,
};

use super::lifecycle::{self, Step};

/// Where a project lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
	Working,
	Archive(i32),
}

/// How far a project got: used for colours, mirrors `print::project_to_style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
	/// ready to be invoiced or nothing to worry about
	Normal,
	Canceled,
	/// event more than 4 weeks ago
	Old,
	/// event 1–28 days ago
	Recent,
	Today,
	/// within the next week
	ThisWeek,
	/// in 8–14 days
	NextWeek,
	/// further ahead
	Later,
	/// no date
	Undated,
}

/// Payment state shown in the `€` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payment {
	Nothing,
	Partial,
	Complete,
}

/// Values for one table row, computed once per load.
#[derive(Debug, Clone)]
pub struct Summary {
	pub name: String,
	pub manager: String,
	pub invoice_number: String,
	pub date: Option<Date<Utc>>,
	pub offer_ready: bool,
	pub offer_file: bool,
	pub invoice_ready: bool,
	pub invoice_file: bool,
	pub payment: Payment,
	pub archive_ready: bool,
	pub amount: String,
	pub urgency: Urgency,
	pub git: GitStatus,
	pub canceled: bool,
	/// other projects (with location) that use the same invoice number in the
	/// same year
	pub duplicate_of: Vec<String>,
	/// the next step of the lifecycle, `None` when archived
	pub next: Option<Step>,
}

pub struct Entry {
	pub project: Project,
	pub location: Location,
	pub summary: Summary,
}

pub struct Broken {
	pub dir: PathBuf,
	pub location: Location,
	pub error: String,
}

pub struct Data {
	pub entries: Vec<Entry>,
	pub broken: Vec<Broken>,
	/// years that have an archive folder
	pub years: Vec<i32>,
	pub templates: Vec<String>,
	pub root: PathBuf,
	pub git: bool,
	pub git_changes: usize,
	pub took: Duration,
}

impl Data {
	pub fn find_by_file(
		&self,
		file: &Path,
	) -> Option<usize> {
		self.entries.iter().position(|e| e.project.file() == file)
	}
}

fn urgency(
	project: &Project,
	invoice_ready: bool,
	today: Date<Utc>,
) -> Urgency {
	if invoice_ready {
		return Urgency::Normal;
	}
	let Some(date) = project.modified_date() else {
		return Urgency::Undated;
	};
	if project.canceled() {
		return Urgency::Canceled;
	}
	match today.signed_duration_since(date).num_days() {
		age if age > 28 => Urgency::Old,
		1..=28 => Urgency::Recent,
		0 => Urgency::Today,
		-7..=-1 => Urgency::ThisWeek,
		-14..=-8 => Urgency::NextWeek,
		_ => Urgency::Later,
	}
}

fn summarize(
	project: &Project,
	location: Location,
	today: Date<Utc>,
) -> Summary {
	let content = std::fs::read_to_string(project.file()).unwrap_or_default();
	let next = lifecycle::lifecycle(&lifecycle::facts(project, &content, location, today)).current;
	let offer_ready = project.is_missing_for_offer().is_empty();
	let invoice_ready = project.is_missing_for_invoice().is_empty();
	let payment = match (project.is_payed(), project.hours().employees_payed()) {
		(true, true) => Payment::Complete,
		(false, false) => Payment::Nothing,
		_ => Payment::Partial,
	};
	Summary {
		name: project.short_desc(),
		manager: project.responsible().unwrap_or("").to_owned(),
		invoice_number: project.invoice().number_str().unwrap_or_default(),
		date: project.modified_date(),
		offer_ready,
		offer_file: project.offer_file_exists(),
		invoice_ready,
		invoice_file: project.invoice_file_exists(),
		payment,
		archive_ready: Project::is_ready_for_archive(project).is_empty(),
		amount: project
			.sum_sold()
			.map(|c| currency_to_string(&c))
			.unwrap_or_else(|_| String::from("–")),
		urgency: urgency(project, invoice_ready, today),
		git: project.get_git_status(),
		canceled: project.canceled(),
		duplicate_of: Vec::new(),
		next,
	}
}

fn open_storage() -> Result<Storage<Project>, Error> {
	match storage::setup_with_git::<Project>() {
		Ok(storage) => Ok(storage),
		Err(error) => {
			log::warn!(
				"{}",
				lformat!("git is not available, continuing without: {}", error)
			);
			storage::setup::<Project>()
		},
	}
}

/// Number of changed files in the storage repository.
#[cfg(feature = "git_statuses")]
fn git_change_count(storage: &Storage<Project>) -> usize {
	storage
		.repository()
		.map(|repo| repo.statuses.keys().filter(|path| path.is_file()).count())
		.unwrap_or(0)
}

#[cfg(not(feature = "git_statuses"))]
fn git_change_count(_storage: &Storage<Project>) -> usize {
	0
}

/// Opens the storage and every project. Runs in the background thread.
pub fn load() -> Result<Data, Error> {
	let started = Instant::now();
	let today = Utc::today();
	let storage = open_storage()?;

	let mut entries = Vec::new();
	let mut broken = Vec::new();

	let mut open_all = |folders: Vec<PathBuf>, location: Location| {
		for dir in folders.into_iter().filter(|d| d.is_dir()) {
			match Project::open_folder(&dir) {
				// the library logs YAML syntax errors and continues with an empty document
				Ok(project) if project.yaml().is_null() => broken.push(Broken {
					dir,
					location,
					error: lformat!("the project file is not valid YAML, see the log"),
				}),
				Ok(mut project) => {
					if let Some(repo) = storage.repository() {
						project.set_git_status(repo.get_status(&dir));
					}
					let summary = summarize(&project, location, today);
					entries.push(Entry {
						project,
						location,
						summary,
					});
				},
				Err(error) => broken.push(Broken {
					dir,
					location,
					error: readable_error(&error.to_string(), storage.root_dir()),
				}),
			}
		}
	};

	open_all(
		storage.list_project_folders(StorageDir::Working)?,
		Location::Working,
	);
	let years = storage.list_years()?;
	for &year in &years {
		open_all(
			storage.list_project_folders(StorageDir::Archive(year))?,
			Location::Archive(year),
		);
	}

	mark_duplicate_invoice_numbers(&mut entries);
	let git_changes = git_change_count(&storage);

	Ok(Data {
		entries,
		broken,
		years,
		templates: storage.list_template_names().unwrap_or_default(),
		root: storage.root_dir().to_owned(),
		git: storage.repository().is_some(),
		git_changes,
		took: started.elapsed(),
	})
}

/// Fills `Summary::duplicate_of` for invoices that share their number with
/// another one of the same year.
fn mark_duplicate_invoice_numbers(entries: &mut [Entry]) {
	let groups = {
		let projects: Vec<&Project> = entries.iter().map(|e| &e.project).collect();
		asciii::stats::duplicate_invoice_numbers(&projects)
	};
	let describe = |e: &Entry| match e.location {
		Location::Working => e.summary.name.clone(),
		Location::Archive(year) => lformat!("{} (archive {})", e.summary.name, year),
	};
	for group in groups {
		let names: Vec<String> = group.iter().map(|&i| describe(&entries[i])).collect();
		for (n, &i) in group.iter().enumerate() {
			entries[i].summary.duplicate_of = names
				.iter()
				.enumerate()
				.filter(|(m, _)| *m != n)
				.map(|(_, name)| name.clone())
				.collect();
		}
	}
}

/// Starts loading in a background thread.
pub fn load_in_background() -> Receiver<Result<Data, String>> {
	let (sender, receiver) = mpsc::channel();
	thread::spawn(move || {
		// the UI only shows the message, including its causes
		let _ = sender.send(load().map_err(|e| format!("{:#}", e)));
	});
	receiver
}

/// Loads the project folders below `root` without a storage, for the render
/// tests.
#[cfg(test)]
pub fn load_folders(root: &Path) -> Data {
	let today = Utc::today();
	let mut entries = Vec::new();
	let mut broken = Vec::new();
	let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
		.map(|d| {
			d.filter_map(|e| e.ok())
				.map(|e| e.path())
				.filter(|p| p.is_dir())
				.collect()
		})
		.unwrap_or_default();
	dirs.sort();
	for dir in dirs {
		match Project::open_folder(&dir) {
			Ok(project) if !project.yaml().is_null() => {
				let summary = summarize(&project, Location::Working, today);
				entries.push(Entry {
					project,
					location: Location::Working,
					summary,
				});
			},
			Ok(_) => broken.push(Broken {
				dir,
				location: Location::Working,
				error: String::from("not valid YAML"),
			}),
			Err(error) => broken.push(Broken {
				dir,
				location: Location::Working,
				error: error.to_string(),
			}),
		}
	}
	mark_duplicate_invoice_numbers(&mut entries);
	Data {
		entries,
		broken,
		years: vec![2025],
		templates: vec![String::from("default")],
		root: root.to_owned(),
		git: false,
		git_changes: 0,
		took: Duration::default(),
	}
}

/// Library errors name folders with their full path, often with a `/./` inside;
/// inside the storage the path relative to it is enough.
fn readable_error(
	error: &str,
	root: &Path,
) -> String {
	let error = error.replace("/./", "/");
	let root = root.to_string_lossy().replace("/./", "/");
	error.replace(&format!("{}/", root.trim_end_matches('/')), "")
}
