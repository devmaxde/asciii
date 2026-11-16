//! The one list of everything the TUI can do.
//!
//! Key dispatch, the footer, the help, the command palette, the Actions panel
//! of the project page and the mouse targets are all derived from this
//! registry, so they cannot disagree.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use asciii::{project::BillType, stats::TaskKind, storage::Storable};

use super::{
	app::{App, Scope, Screen},
	data::Location,
	lifecycle::{self, Step},
	offer_editor::EditorMode,
	ops::GitOp,
	template_editor::After,
};

// ── keys ────────────────────────────────────────────────────────────────────

/// A key as the registry knows it: a key code, with or without Ctrl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
	pub code: KeyCode,
	pub ctrl: bool,
}

impl Key {
	pub const fn ch(c: char) -> Key {
		Key {
			code: KeyCode::Char(c),
			ctrl: false,
		}
	}

	pub const fn ctrl(c: char) -> Key {
		Key {
			code: KeyCode::Char(c),
			ctrl: true,
		}
	}

	pub const fn code(code: KeyCode) -> Key {
		Key { code, ctrl: false }
	}

	/// Shift is part of the character (`I`, `?`), so it is not compared; Alt
	/// never matches.
	pub fn matches(
		self,
		event: &KeyEvent,
	) -> bool {
		let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
		if event.modifiers.contains(KeyModifiers::ALT) || ctrl != self.ctrl {
			return false;
		}
		match (self.code, event.code) {
			(KeyCode::Char(a), KeyCode::Char(b)) if self.ctrl => a.eq_ignore_ascii_case(&b),
			(a, b) => a == b,
		}
	}

	/// How the key is written on screen: `Shift+I`, `Ctrl+P`, `Enter`, `↑`.
	pub fn label(self) -> String {
		let base = match self.code {
			KeyCode::Char(' ') => String::from("Space"),
			KeyCode::Char(c) if self.ctrl => c.to_ascii_uppercase().to_string(),
			KeyCode::Char(c) if c.is_uppercase() => format!("Shift+{}", c),
			KeyCode::Char(c) => c.to_string(),
			KeyCode::Enter => String::from("Enter"),
			KeyCode::Esc => String::from("Esc"),
			KeyCode::Tab => String::from("Tab"),
			KeyCode::BackTab => String::from("Shift+Tab"),
			KeyCode::Backspace => String::from("Backspace"),
			KeyCode::Delete => String::from("Del"),
			KeyCode::Up => String::from("↑"),
			KeyCode::Down => String::from("↓"),
			KeyCode::Left => String::from("←"),
			KeyCode::Right => String::from("→"),
			KeyCode::PageUp => String::from("PgUp"),
			KeyCode::PageDown => String::from("PgDn"),
			KeyCode::Home => String::from("Home"),
			KeyCode::End => String::from("End"),
			KeyCode::F(n) => format!("F{}", n),
			other => format!("{:?}", other),
		};
		if self.ctrl {
			format!("Ctrl+{}", base)
		} else {
			base
		}
	}
}

const ESC: Key = Key::code(KeyCode::Esc);
const ENTER: Key = Key::code(KeyCode::Enter);
const LEFT: Key = Key::code(KeyCode::Left);
const RIGHT: Key = Key::code(KeyCode::Right);

// ── contexts ────────────────────────────────────────────────────────────────

/// Where the user is; decides which actions exist and what their keys are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
	Dashboard,
	Projects,
	/// the project list showing folders that could not be opened
	Broken,
	Detail,
	Log,
	Offer,
	Usage,
	/// the Template tab
	Template,
	/// no projects loaded: still loading, or the storage could not be opened
	Empty,
}

impl Ctx {
	pub const ALL: [Ctx; 9] = [
		Ctx::Dashboard,
		Ctx::Projects,
		Ctx::Broken,
		Ctx::Detail,
		Ctx::Log,
		Ctx::Offer,
		Ctx::Usage,
		Ctx::Template,
		Ctx::Empty,
	];

	/// The tabs (and the page shown instead when nothing is loaded): `q` quits
	/// here.
	pub fn is_top(self) -> bool {
		matches!(
			self,
			Ctx::Dashboard | Ctx::Projects | Ctx::Broken | Ctx::Log | Ctx::Template | Ctx::Empty
		)
	}

	fn is_editor(self) -> bool {
		matches!(self, Ctx::Offer | Ctx::Usage)
	}

	/// Screens that browse projects (not the editor, not the empty page).
	fn browses(self) -> bool {
		matches!(
			self,
			Ctx::Dashboard | Ctx::Projects | Ctx::Broken | Ctx::Detail | Ctx::Log
		)
	}

	pub fn title(self) -> String {
		match self {
			Ctx::Dashboard => lformat!("Dashboard"),
			Ctx::Projects => lformat!("Project list"),
			Ctx::Broken => lformat!("Broken projects"),
			Ctx::Detail => lformat!("Project page"),
			Ctx::Log => lformat!("Log"),
			Ctx::Offer => lformat!("Offer editor"),
			Ctx::Usage => lformat!("Actual usage editor"),
			Ctx::Template => lformat!("Template"),
			Ctx::Empty => lformat!("Start page"),
		}
	}
}

/// Sections of the help and the Actions panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
	Navigate,
	List,
	Offer,
	Invoice,
	Project,
	Files,
	Editor,
	Template,
	Export,
	Git,
	App,
}

impl Group {
	pub fn label(self) -> String {
		match self {
			Group::Navigate => lformat!("Navigate"),
			Group::List => lformat!("List"),
			Group::Offer => lformat!("Offer"),
			Group::Invoice => lformat!("Invoice"),
			Group::Project => lformat!("Project"),
			Group::Files => lformat!("Files"),
			Group::Editor => lformat!("Editor"),
			Group::Template => lformat!("Template"),
			Group::Export => lformat!("Export"),
			Group::Git => lformat!("Git"),
			Group::App => lformat!("App"),
		}
	}
}

/// Whether an action can run now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Avail {
	Enabled,
	/// shown greyed out, with the reason
	Disabled(String),
	/// does not apply here
	Hidden,
}

impl Avail {
	pub fn is_enabled(&self) -> bool {
		matches!(self, Avail::Enabled)
	}
}

// ── actions ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
	// app and navigation
	Palette,
	Help,
	Quit,
	Back,
	Dashboard,
	Projects,
	TemplateTab,
	Log,
	NextTab,
	PrevTab,
	YearPrev,
	YearNext,
	Reload,
	ToggleMouse,
	CreateStorage,
	// lists
	Open,
	Search,
	Scope,
	ScopeYear,
	ScopeAll,
	ScopeBroken,
	ToggleArchived,
	Sort,
	// the selected project
	ShowActions,
	NewProject,
	Details,
	OfferEditor,
	MakeOffer,
	Accepted,
	UsageEditor,
	InvoiceNumber,
	SetInvoiceDate,
	MakeInvoice,
	SetPaidDate,
	SetWagesDate,
	MakeMenu,
	EditFile,
	OpenFolder,
	Archive,
	Delete,
	// offer and usage editor
	More,
	Less,
	AsOffered,
	NoneUsed,
	ChangeLine,
	AddProduct,
	NewProduct,
	RemoveLine,
	Save,
	// the Template tab
	TemplatePrev,
	TemplateNext,
	ClearUsage,
	EditTemplateFile,
	// export and git
	ExportCsv,
	ExportIcs,
	GitMenu,
	GitStatus,
	GitDiff,
	GitLog,
	GitAdd,
	GitCommit,
	GitPull,
	GitPush,
}

use Action::*;

impl Action {
	/// Every action, in the order of the help and the palette.
	pub const ALL: [Action; 62] = [
		Open,
		Back,
		ShowActions,
		Search,
		Scope,
		ToggleArchived,
		Sort,
		YearPrev,
		YearNext,
		NewProject,
		Details,
		OfferEditor,
		MakeOffer,
		Accepted,
		UsageEditor,
		InvoiceNumber,
		SetInvoiceDate,
		MakeInvoice,
		SetPaidDate,
		SetWagesDate,
		MakeMenu,
		Archive,
		Delete,
		EditFile,
		OpenFolder,
		More,
		Less,
		AsOffered,
		NoneUsed,
		ChangeLine,
		AddProduct,
		NewProduct,
		RemoveLine,
		Save,
		TemplatePrev,
		TemplateNext,
		ClearUsage,
		EditTemplateFile,
		Dashboard,
		Projects,
		TemplateTab,
		Log,
		NextTab,
		PrevTab,
		ScopeYear,
		ScopeAll,
		ScopeBroken,
		ExportCsv,
		ExportIcs,
		GitMenu,
		GitStatus,
		GitDiff,
		GitLog,
		GitAdd,
		GitCommit,
		GitPull,
		GitPush,
		Reload,
		CreateStorage,
		ToggleMouse,
		Palette,
		Help,
	];

	/// Quitting is listed last in the help.
	pub fn all() -> impl Iterator<Item = Action> {
		Action::ALL.into_iter().chain(std::iter::once(Quit))
	}

	/// Whether the action exists in `ctx` at all.
	pub fn in_ctx(
		self,
		ctx: Ctx,
	) -> bool {
		use Ctx as C;
		match self {
			Palette | Help | Quit | ToggleMouse => true,
			Back => matches!(
				ctx,
				C::Detail | C::Offer | C::Usage | C::Projects | C::Broken | C::Template
			),
			Dashboard | Projects | NextTab | PrevTab => ctx.browses() || ctx == C::Template,
			Log | TemplateTab => ctx.browses() || matches!(ctx, C::Empty | C::Template),
			YearPrev | YearNext => matches!(ctx, C::Dashboard | C::Projects | C::Broken),
			Reload => ctx.browses() || matches!(ctx, C::Empty | C::Template),
			CreateStorage => matches!(ctx, C::Empty | C::Template),
			Open => matches!(ctx, C::Dashboard | C::Projects | C::Broken),
			Search => matches!(ctx, C::Projects | C::Template),
			ToggleArchived | Sort => ctx == C::Projects,
			Scope => matches!(ctx, C::Projects | C::Broken),
			ScopeYear | ScopeAll | ScopeBroken => ctx.browses(),
			ShowActions => ctx == C::Detail,
			NewProject => ctx.browses(),
			Details | InvoiceNumber | Delete => {
				matches!(ctx, C::Dashboard | C::Projects | C::Broken | C::Detail)
			},
			OfferEditor | MakeOffer | Accepted | UsageEditor | SetInvoiceDate | MakeInvoice
			| SetPaidDate | SetWagesDate | MakeMenu | OpenFolder | Archive => ctx == C::Detail,
			EditFile => matches!(ctx, C::Detail | C::Broken),
			More | Less | ChangeLine | NewProduct | RemoveLine | Save => {
				ctx.is_editor() || ctx == C::Template
			},
			AddProduct => ctx.is_editor(),
			TemplatePrev | TemplateNext | ClearUsage | EditTemplateFile => ctx == C::Template,
			AsOffered | NoneUsed => ctx == C::Usage,
			ExportCsv | ExportIcs | GitMenu | GitStatus | GitDiff | GitLog | GitAdd | GitCommit
			| GitPull | GitPush => ctx.browses(),
		}
	}

	/// The keys of the action in `ctx`; the first one is shown in the footer and
	/// the panel.
	pub fn keys(
		self,
		ctx: Ctx,
	) -> &'static [Key] {
		use Ctx as C;
		const NONE: &[Key] = &[];
		match self {
			Palette => const { &[Key::ctrl('p'), Key::code(KeyCode::F(1)), Key::ch(':')] },
			Help => const { &[Key::ch('?')] },
			Quit if ctx.is_top() => const { &[Key::ch('q'), Key::ctrl('q'), Key::ctrl('c')] },
			Quit => const { &[Key::ctrl('q'), Key::ctrl('c')] },
			Back => match ctx {
				C::Detail => const { &[ESC, Key::ch('q'), Key::code(KeyCode::Backspace)] },
				C::Offer | C::Usage => const { &[ESC, Key::ch('q')] },
				_ => const { &[ESC] },
			},
			Dashboard => const { &[Key::ch('1')] },
			Projects => const { &[Key::ch('2')] },
			TemplateTab => const { &[Key::ch('3')] },
			Log => const { &[Key::ch('4')] },
			NextTab => const { &[Key::code(KeyCode::Tab)] },
			PrevTab => const { &[Key::code(KeyCode::BackTab)] },
			YearPrev => const { &[LEFT, Key::ch('h')] },
			YearNext => const { &[RIGHT, Key::ch('l')] },
			Reload => const { &[Key::ch('r')] },
			Open => const { &[ENTER] },
			Search => const { &[Key::ch('/')] },
			Scope => const { &[Key::ch('w')] },
			ToggleArchived => const { &[Key::ch('a')] },
			Sort => const { &[Key::ch('s')] },
			ShowActions => const { &[ENTER] },
			NewProject => const { &[Key::ch('n')] },
			Details => const { &[Key::ch('p')] },
			OfferEditor => const { &[Key::ch('o')] },
			Accepted => const { &[Key::ch('A')] },
			UsageEditor => const { &[Key::ch('u')] },
			InvoiceNumber => const { &[Key::ch('I')] },
			MakeMenu => const { &[Key::ch('m')] },
			EditFile => const { &[Key::ch('e')] },
			Archive => const { &[Key::ch('a')] },
			Delete => const { &[Key::ch('D')] },
			More if ctx == C::Template => const { &[Key::ch('+'), Key::ch('=')] },
			Less if ctx == C::Template => const { &[Key::ch('-')] },
			More if ctx == C::Offer => const { &[Key::ch('+'), RIGHT, Key::ch('=')] },
			More => const { &[Key::ch('+'), RIGHT] },
			Less => const { &[Key::ch('-'), LEFT] },
			AsOffered => const { &[Key::ch('=')] },
			NoneUsed => const { &[Key::ch('0')] },
			ChangeLine => const { &[ENTER, Key::ch('e')] },
			AddProduct => const { &[Key::ch('a')] },
			NewProduct => const { &[Key::ch('n')] },
			RemoveLine => const { &[Key::ch('d'), Key::code(KeyCode::Delete)] },
			Save => const { &[Key::ch('s'), Key::ctrl('s')] },
			ExportCsv => const { &[Key::ch('x')] },
			ExportIcs => const { &[Key::ch('c')] },
			GitMenu => const { &[Key::ch('g')] },
			CreateStorage => const { &[Key::ch('c')] },
			TemplatePrev => const { &[LEFT, Key::ch('h')] },
			TemplateNext => const { &[RIGHT, Key::ch('l')] },
			ClearUsage => const { &[Key::ch('u')] },
			EditTemplateFile => const { &[Key::ch('E')] },
			ScopeYear | ScopeAll | ScopeBroken | ToggleMouse | MakeOffer | MakeInvoice
			| SetInvoiceDate | SetPaidDate | SetWagesDate | OpenFolder | GitStatus | GitDiff | GitLog
			| GitAdd | GitCommit | GitPull | GitPush => NONE,
		}
	}

	pub fn group(self) -> Group {
		match self {
			Open | Back | ShowActions | Dashboard | Projects | TemplateTab | Log | NextTab | PrevTab
			| YearPrev | YearNext => Group::Navigate,
			Search | Scope | ScopeYear | ScopeAll | ScopeBroken | ToggleArchived | Sort => Group::List,
			NewProject | Archive | Delete => Group::Project,
			Details | OfferEditor | MakeOffer | Accepted => Group::Offer,
			UsageEditor | InvoiceNumber | SetInvoiceDate | MakeInvoice | SetPaidDate | SetWagesDate
			| MakeMenu => Group::Invoice,
			EditFile | OpenFolder => Group::Files,
			More | Less | AsOffered | NoneUsed | ChangeLine | AddProduct | NewProduct | RemoveLine
			| Save => Group::Editor,
			TemplatePrev | TemplateNext | ClearUsage | EditTemplateFile => Group::Template,
			ExportCsv | ExportIcs => Group::Export,
			GitMenu | GitStatus | GitDiff | GitLog | GitAdd | GitCommit | GitPull | GitPush => Group::Git,
			Palette | Help | Quit | Reload | ToggleMouse | CreateStorage => Group::App,
		}
	}

	/// Short name, used everywhere.
	pub fn label(self) -> String {
		match self {
			Palette => lformat!("Commands"),
			Help => lformat!("Help"),
			Quit => lformat!("Quit"),
			Back => lformat!("Back"),
			Dashboard => lformat!("Go to the dashboard"),
			Projects => lformat!("Go to the project list"),
			Log => lformat!("Go to the log"),
			TemplateTab => lformat!("Edit the template (catalog and starting offer)"),
			NextTab => lformat!("Next tab"),
			PrevTab => lformat!("Previous tab"),
			YearPrev => lformat!("Previous year"),
			YearNext => lformat!("Next year"),
			Reload => lformat!("Reload projects"),
			ToggleMouse => lformat!("Mouse: on/off"),
			CreateStorage => lformat!("Create empty project folder"),
			Open => lformat!("Open"),
			Search => lformat!("Search"),
			Scope => lformat!("Show: year / all / broken"),
			ScopeYear => lformat!("Show projects of the selected year"),
			ScopeAll => lformat!("Show projects of all years"),
			ScopeBroken => lformat!("Show broken projects"),
			ToggleArchived => lformat!("Show archived: on/off"),
			Sort => lformat!("Sort"),
			ShowActions => lformat!("Actions"),
			NewProject => lformat!("New project"),
			Details => lformat!("Project details (client, dates, …)"),
			OfferEditor => lformat!("Edit the offer"),
			MakeOffer => lformat!("Create the offer PDF"),
			Accepted => lformat!("Client accepted the offer"),
			UsageEditor => lformat!("Note the actual usage"),
			InvoiceNumber => lformat!("Assign the invoice number"),
			SetInvoiceDate => lformat!("Set the invoice date"),
			MakeInvoice => lformat!("Create the invoice PDF"),
			SetPaidDate => lformat!("Client paid: set the date"),
			SetWagesDate => lformat!("Wages paid: set the date"),
			MakeMenu => lformat!("Create a PDF"),
			EditFile => lformat!("Edit the project file"),
			OpenFolder => lformat!("Open the project folder"),
			Archive => lformat!("Archive the project"),
			Delete => lformat!("Delete the project"),
			More => lformat!("More"),
			Less => lformat!("Less"),
			AsOffered => lformat!("Used as offered"),
			NoneUsed => lformat!("Nothing used"),
			ChangeLine => lformat!("Change the product"),
			AddProduct => lformat!("Add from the catalog"),
			NewProduct => lformat!("New product"),
			RemoveLine => lformat!("Remove the product"),
			Save => lformat!("Save"),
			TemplatePrev => lformat!("Previous template"),
			TemplateNext => lformat!("Next template"),
			ClearUsage => lformat!("Clear the usage numbers"),
			EditTemplateFile => lformat!("Edit the template file"),
			ExportCsv => lformat!("Export the year as CSV"),
			ExportIcs => lformat!("Export the year as calendar (.ics)"),
			GitMenu => lformat!("Git"),
			GitStatus => lformat!("Git: show changed files"),
			GitDiff => lformat!("Git: show changes"),
			GitLog => lformat!("Git: show history"),
			GitAdd => lformat!("Git: stage all changes"),
			GitCommit => lformat!("Git: commit"),
			GitPull => lformat!("Git: pull from the remote"),
			GitPush => lformat!("Git: push to the remote"),
		}
	}

	/// The label for the current state, e.g. "Move back from the archive" instead
	/// of "Archive the project".
	pub fn title(
		self,
		app: &App,
	) -> String {
		match self {
			Archive
				if app
					.selected_entry()
					.is_some_and(|e| matches!(e.location, Location::Archive(_))) =>
			{
				lformat!("Move back from the archive")
			},
			Accepted
				if app
					.selected_entry()
					.is_some_and(|e| app.is_accepted(&e.project)) =>
			{
				lformat!("Take back: offer accepted")
			},
			Open if app.screen == Screen::Dashboard => lformat!("Open the to-do"),
			Open if app.scope == Scope::Broken => lformat!("Why is it broken?"),
			Open => lformat!("Open the project"),
			Reload if app.screen == Screen::Template => lformat!("Read the template again"),
			CreateStorage if app.screen == Screen::Template && app.storage_missing().is_none() => {
				lformat!("Create the default template")
			},
			Reload if app.data.is_none() => lformat!("Try again"),
			Save if app.screen == Screen::Template => lformat!("Save the template"),
			More if app.screen == Screen::Template => lformat!("One more in the starting offer"),
			Less if app.screen == Screen::Template => lformat!("One less in the starting offer"),
			ToggleMouse if app.mouse => lformat!("Mouse: off (for selecting text)"),
			ToggleMouse => lformat!("Mouse: on"),
			_ => self.label(),
		}
	}

	/// One sentence for the help and the Actions panel.
	pub fn description(self) -> String {
		match self {
			Palette => lformat!("search every command and project by name"),
			Help => lformat!("this help"),
			Quit => lformat!("leave asciii (asks first if the offer has unsaved changes)"),
			Back => lformat!("one step back: close the page, the editor or the search"),
			Dashboard | Projects | TemplateTab | Log | NextTab | PrevTab => {
				lformat!("switch between dashboard, projects, template and log")
			},
			YearPrev | YearNext => lformat!("the year shown on the dashboard and in the list"),
			Reload => lformat!("read all projects from disk again"),
			ToggleMouse => lformat!("with the mouse off, the terminal selects text as usual"),
			CreateStorage => {
				lformat!("create the folders working, archive and templates with a default template")
			},
			Open => lformat!("open the highlighted line"),
			Search => lformat!("filter by name, invoice number or manager"),
			Scope | ScopeYear | ScopeAll | ScopeBroken => {
				lformat!("projects of the year, of all years, or folders that cannot be read")
			},
			ToggleArchived => lformat!("show or hide archived projects in the list"),
			Sort => lformat!("by date or bill number: ascending, descending, then the other one"),
			ShowActions => lformat!("what can be done with this project; Enter runs the highlighted one"),
			NewProject => lformat!("create a project from a template"),
			Details => lformat!("fill in client, dates, invoice number and manager in one form"),
			OfferEditor => lformat!("add, remove and change products without editing the file"),
			MakeOffer => lformat!("write the offer as PDF into the output folder"),
			Accepted => lformat!("note that the client accepted the offer, or take it back"),
			UsageEditor => lformat!("after the catering: what was actually used, returned or extra"),
			InvoiceNumber => lformat!("the next free invoice number of the year"),
			SetInvoiceDate => lformat!("the date printed on the invoice"),
			MakeInvoice => lformat!("write the invoice as PDF into the output folder"),
			SetPaidDate => lformat!("the day the client paid the invoice"),
			SetWagesDate => lformat!("the day the staff got their wages"),
			MakeMenu => lformat!("create the offer or invoice PDF (pdflatex)"),
			EditFile => lformat!("open the project file in your editor"),
			OpenFolder => lformat!("show the project folder in the file manager"),
			Archive => lformat!("move the finished project into the archive of its year, or back"),
			Delete => lformat!("delete the project folder (asks for the name)"),
			More | Less => lformat!("more or less of the highlighted product"),
			AsOffered => lformat!("everything that was offered was used"),
			NoneUsed => lformat!("nothing of the highlighted product was used"),
			ChangeLine => lformat!("change name, unit, price and amount"),
			AddProduct => lformat!("a product of the project catalog or the template"),
			NewProduct => lformat!("a product that is in no catalog yet"),
			RemoveLine => lformat!("remove the highlighted product"),
			Save => lformat!("write the changes into the project file"),
			TemplatePrev | TemplateNext => {
				lformat!("choose the template to edit, when there are several")
			},
			ClearUsage => {
				lformat!("new projects start without sold or returned numbers for the highlighted product")
			},
			EditTemplateFile => lformat!("open the template file in your editor"),
			ExportCsv => lformat!("all projects of the year as a table, into the output folder"),
			ExportIcs => lformat!("all events of the year as a calendar file, into the output folder"),
			GitMenu => lformat!("status, diff, history, commit, pull, push"),
			GitStatus | GitDiff | GitLog | GitAdd | GitCommit | GitPull | GitPush => {
				lformat!("the project folder is a git repository")
			},
		}
	}

	/// The description in `ctx`: the editing keys mean the template there.
	pub fn description_in(
		self,
		ctx: Ctx,
	) -> String {
		match self {
			_ if ctx != Ctx::Template => self.description(),
			More | Less => lformat!("how many of the highlighted product new projects offer"),
			ChangeLine => lformat!("change name, unit, price and starting quantity"),
			NewProduct => lformat!("a new product in the catalog of the template"),
			RemoveLine => lformat!("remove the product from the template (asks first)"),
			Save => lformat!("write the changes into the template file"),
			Search => lformat!("filter the catalog by name"),
			Reload => lformat!("read the template from disk again"),
			_ => self.description(),
		}
	}

	/// Footer position in `ctx`: lower comes first and is dropped last. `None`:
	/// not in the footer. Help and the palette are not listed, the footer always
	/// ends with them.
	pub fn footer(
		self,
		ctx: Ctx,
	) -> Option<u8> {
		use Ctx as C;
		let priority = match (ctx, self) {
			(C::Dashboard, Open) => 1,
			(C::Dashboard, NewProject) => 2,
			(C::Dashboard, YearPrev) => 3,
			(C::Dashboard, NextTab) => 4,
			(C::Dashboard, Quit) => 9,
			(C::Projects, Back) => 0,
			(C::Projects, Open) => 1,
			(C::Projects, Search) => 2,
			(C::Projects, NewProject) => 3,
			(C::Projects, Scope) => 4,
			(C::Projects, ToggleArchived) => 5,
			(C::Projects, Sort) => 6,
			(C::Projects, YearPrev) => 7,
			(C::Projects, Quit) => 9,
			(C::Broken, Back) => 0,
			(C::Broken, Open) => 1,
			(C::Broken, EditFile) => 2,
			(C::Broken, Scope) => 3,
			(C::Broken, Quit) => 9,
			(C::Detail, Back) => 0,
			(C::Detail, ShowActions) => 1,
			(C::Detail, OfferEditor) => 2,
			(C::Detail, Details) => 3,
			(C::Detail, MakeMenu) => 4,
			(C::Detail, InvoiceNumber) => 5,
			(C::Detail, UsageEditor) => 6,
			(C::Detail, Accepted) => 7,
			(C::Detail, Archive) => 8,
			(C::Detail, EditFile) => 9,
			(C::Log, NextTab) => 1,
			(C::Log, Quit) => 9,
			(C::Offer | C::Usage, Back) => 0,
			(C::Offer | C::Usage, Save) => 1,
			(C::Offer | C::Usage, More) => 2,
			(C::Offer | C::Usage, ChangeLine) => 3,
			(C::Offer | C::Usage, AddProduct) => 4,
			(C::Offer | C::Usage, NewProduct) => 5,
			(C::Usage, AsOffered) => 6,
			(C::Offer | C::Usage, RemoveLine) => 7,
			(C::Template, Back) => 0,
			(C::Template, CreateStorage) => 1,
			(C::Template, Save) => 1,
			(C::Template, More) => 2,
			(C::Template, ChangeLine) => 3,
			(C::Template, NewProduct) => 4,
			(C::Template, RemoveLine) => 5,
			(C::Template, TemplatePrev) => 6,
			(C::Template, Search) => 7,
			(C::Template, ClearUsage) => 8,
			(C::Template, EditTemplateFile) => 9,
			(C::Template, Quit) => 10,
			(C::Empty, Reload) => 1,
			(C::Empty, CreateStorage) => 2,
			(C::Empty, Log) => 3,
			(C::Empty, Quit) => 4,
			_ => return None,
		};
		Some(priority)
	}

	/// Short footer text: keys and label, e.g. `←/→ year`. Pairs (year, amount)
	/// share one entry.
	pub fn footer_text(
		self,
		ctx: Ctx,
	) -> (String, String) {
		match self {
			YearPrev => (String::from("←/→"), lformat!("year")),
			More if ctx == Ctx::Usage => (String::from("+/-"), lformat!("used")),
			More if ctx == Ctx::Template => (String::from("+/-"), lformat!("starting offer")),
			More => (String::from("+/-"), lformat!("amount")),
			TemplatePrev => (String::from("←/→"), lformat!("template")),
			NextTab => (Key::code(KeyCode::Tab).label(), lformat!("switch tab")),
			_ => {
				let key = self
					.keys(ctx)
					.first()
					.map(|k| k.label())
					.unwrap_or_default();
				let label = match self {
					Back => lformat!("back"),
					Open => lformat!("open"),
					Search => lformat!("search"),
					NewProject => lformat!("new"),
					Scope => lformat!("show"),
					ToggleArchived => lformat!("archived"),
					Sort => lformat!("sort"),
					ShowActions => lformat!("actions"),
					OfferEditor => lformat!("offer"),
					Details => lformat!("details"),
					MakeMenu => lformat!("PDF"),
					InvoiceNumber => lformat!("invoice no."),
					UsageEditor => lformat!("usage"),
					Accepted => lformat!("accepted"),
					Archive => lformat!("archive"),
					EditFile | EditTemplateFile => lformat!("edit file"),
					ClearUsage => lformat!("clear usage"),
					Save => lformat!("save"),
					ChangeLine => lformat!("change"),
					AddProduct => lformat!("add"),
					NewProduct => lformat!("new product"),
					RemoveLine => lformat!("remove"),
					AsOffered => lformat!("as offered"),
					Quit => lformat!("quit"),
					Reload => lformat!("try again"),
					CreateStorage => lformat!("create folder"),
					Log => lformat!("log"),
					Help => lformat!("help"),
					Palette => lformat!("commands"),
					_ => self.label(),
				};
				(key, label)
			},
		}
	}

	/// Whether the command palette lists it. Movement and pairs are left out.
	pub fn in_palette(self) -> bool {
		!matches!(
			self,
			Palette
				| Back
				| NextTab
				| PrevTab
				| ShowActions
				| More
				| Less
				| Open
				| TemplatePrev
				| TemplateNext
		)
	}
}

// ── availability ────────────────────────────────────────────────────────────

impl Action {
	pub fn availability(
		self,
		app: &App,
	) -> Avail {
		let ctx = app.ctx();
		if !self.in_ctx(ctx) {
			return Avail::Hidden;
		}
		let disabled = |reason: String| Avail::Disabled(reason);
		let loaded = app.data.is_some();
		let entry = app.selected_entry();
		let archived = entry.is_some_and(|e| matches!(e.location, Location::Archive(_)));
		let read_only = || lformat!("the project is archived; move it back first (a)");
		let editor = app.offer.as_ref();
		if ctx == Ctx::Template {
			if let Some(avail) = self.in_template(app) {
				return avail;
			}
		}

		match self {
			Back => match ctx {
				Ctx::Projects | Ctx::Broken if app.search.is_empty() && !app.searching => Avail::Hidden,
				_ => Avail::Enabled,
			},
			YearPrev | YearNext | ScopeYear | ScopeAll | ScopeBroken | Search | Scope
			| ToggleArchived | Sort | ExportCsv | ExportIcs
				if !loaded =>
			{
				disabled(lformat!("no projects are loaded"))
			},
			Reload if app.is_loading() => disabled(lformat!("loading …")),
			CreateStorage => match app.storage_missing() {
				Some(_) => Avail::Enabled,
				None => Avail::Hidden,
			},
			ScopeBroken if app.data.as_ref().is_some_and(|d| d.broken.is_empty()) => {
				disabled(lformat!("all project folders can be read"))
			},
			Open => match ctx {
				Ctx::Dashboard
					if app
						.task_state
						.selected()
						.is_none_or(|t| t >= app.tasks.len()) =>
				{
					disabled(lformat!("nothing to do"))
				},
				Ctx::Projects | Ctx::Broken if app.table.selected().is_none_or(|r| r >= app.rows.len()) => {
					disabled(lformat!("no project in the list"))
				},
				_ => Avail::Enabled,
			},
			NewProject if !loaded => disabled(lformat!("no projects are loaded")),
			NewProject if app.data.as_ref().is_some_and(|d| d.templates.is_empty()) => {
				disabled(lformat!("there is no template in the templates folder"))
			},

			// the selected project
			Details | InvoiceNumber | Delete if ctx == Ctx::Broken => disabled(lformat!(
				"this folder cannot be read as a project; Enter shows why"
			)),
			EditFile if ctx == Ctx::Broken => {
				if app.selected_broken().is_none() {
					disabled(lformat!("no project in the list"))
				} else if !app.tools.editor {
					disabled(no_editor())
				} else {
					Avail::Enabled
				}
			},
			Details | OfferEditor | MakeOffer | Accepted | UsageEditor | InvoiceNumber
			| SetInvoiceDate | MakeInvoice | SetPaidDate | SetWagesDate | MakeMenu | EditFile
			| OpenFolder | Archive | Delete
				if entry.is_none() =>
			{
				disabled(lformat!("select a project first"))
			},
			Details | OfferEditor | Accepted | UsageEditor | InvoiceNumber | SetInvoiceDate
			| SetPaidDate | SetWagesDate
				if archived =>
			{
				disabled(read_only())
			},
			Details if app.selected_fields().is_empty() => disabled(lformat!(
				"the file has none of the fields this form fills in; use Edit file"
			)),
			InvoiceNumber => match app.selected_value("invoice/number") {
				Some(number) => disabled(lformat!("the invoice number is already set ({})", number)),
				None if !app.selected_has_field("invoice/number") => missing_field("invoice/number"),
				None => Avail::Enabled,
			},
			SetInvoiceDate if !app.selected_has_field("invoice/date") => missing_field("invoice/date"),
			SetPaidDate if !app.selected_has_field("invoice/payed_date") => {
				missing_field("invoice/payed_date")
			},
			SetWagesDate if !app.selected_has_field("hours/wages_date") => {
				missing_field("hours/wages_date")
			},
			MakeOffer | MakeInvoice | MakeMenu => match &app.tools.pdf {
				Err(reason) => disabled(reason.clone()),
				Ok(()) if self == MakeInvoice && app.selected_value("invoice/number").is_none() => {
					disabled(lformat!("assign the invoice number first"))
				},
				Ok(()) => Avail::Enabled,
			},
			EditFile if !app.tools.editor => disabled(no_editor()),

			// editor
			More | Less | AsOffered | NoneUsed | ChangeLine | RemoveLine
				if editor.is_none_or(|e| e.selected().is_none()) =>
			{
				disabled(lformat!("no product selected"))
			},
			AddProduct if editor.is_some_and(|e| e.addable().is_empty()) => disabled(lformat!(
				"every catalog product is in the offer already; use New product"
			)),

			GitMenu | GitStatus | GitDiff | GitLog | GitAdd | GitCommit | GitPull | GitPush => {
				if !loaded {
					disabled(lformat!("no projects are loaded"))
				} else if app.data.as_ref().is_some_and(|d| !d.git) {
					disabled(lformat!("the project folder is not a git repository"))
				} else if !app.tools.git {
					disabled(lformat!("git is not installed"))
				} else {
					Avail::Enabled
				}
			},
			_ => Avail::Enabled,
		}
	}
}

impl Action {
	/// Availability on the Template tab; `None`: the general rules apply.
	fn in_template(
		self,
		app: &App,
	) -> Option<Avail> {
		let disabled = |reason: String| Some(Avail::Disabled(reason));
		let Some(template) = app.template.as_ref() else {
			return matches!(
				self,
				Back
					| Save
					| More
					| Less
					| ChangeLine
					| NewProduct
					| RemoveLine
					| Search
					| ClearUsage
					| EditTemplateFile
					| TemplatePrev
					| TemplateNext
			)
			.then_some(Avail::Hidden);
		};
		let dirty = template.dirty();
		let unsaved = || lformat!("save or discard the changes first (s, or Esc)");
		let usable = template.file.is_some() && template.problem.is_none();
		let row = template.selected();
		let line = row
			.and_then(|r| template.line_of(r))
			.map(|i| &template.offer.lines[i]);
		match self {
            Back if dirty || template.searching || !template.search.is_empty() => Some(Avail::Enabled),
            Back => Some(Avail::Hidden),
            CreateStorage if template.names.is_empty() && template.problem.is_none() => Some(Avail::Enabled),
            TemplatePrev | TemplateNext if template.names.len() < 2 => Some(Avail::Hidden),
            TemplatePrev | TemplateNext | Reload if dirty => disabled(unsaved()),
            EditTemplateFile if template.file.is_none() => disabled(lformat!("there is no template to edit")),
            EditTemplateFile if dirty => disabled(unsaved()),
            Save | NewProduct | Search | More | Less | ChangeLine | RemoveLine | ClearUsage if !usable => {
                disabled(lformat!("there is no template to edit"))
            },
            Save if !dirty => disabled(lformat!("nothing changed")),
            NewProduct if !template.has_catalog && !template.has_products => {
                disabled(lformat!("the template has no catalog section; add \"cataloge:\" with Edit the template file (Shift+E)"))
            },
            More | Less | ChangeLine | RemoveLine | ClearUsage if row.is_none() => disabled(lformat!("no product selected")),
            More | Less if !template.has_products => {
                disabled(lformat!("the template has no products section for a starting offer; add \"products:\" in the editor"))
            },
            Less if line.is_none() => disabled(lformat!("it is not in the starting offer")),
            ClearUsage if line.is_none_or(|l| l.sold.is_none() && l.returned.is_none()) => {
                disabled(lformat!("it starts without sold or returned numbers"))
            },
            EditTemplateFile if !app.tools.editor => disabled(no_editor()),
            Search | Save | NewProduct | More | Less | ChangeLine | RemoveLine | ClearUsage | EditTemplateFile | TemplatePrev
            | TemplateNext => Some(Avail::Enabled),
            _ => None,
        }
	}
}

fn no_editor() -> String {
	lformat!("no editor found: set $EDITOR, e.g. to \"code --wait\"")
}

fn missing_field(path: &str) -> Avail {
	Avail::Disabled(lformat!(
		"\"{}\" is not in the project file; use Edit file",
		super::yaml_edit::label(path)
	))
}

// ── lookup and running ──────────────────────────────────────────────────────

/// The action bound to `key` in `ctx`, if any (whether it is available or not).
pub fn for_key(
	ctx: Ctx,
	key: &KeyEvent,
) -> Option<Action> {
	Action::all().find(|a| a.in_ctx(ctx) && a.keys(ctx).iter().any(|k| k.matches(key)))
}

impl App {
	/// Runs `action` if it is available, otherwise says why not.
	pub fn run(
		&mut self,
		action: Action,
	) {
		match action.availability(self) {
			Avail::Enabled => self.execute(action),
			Avail::Disabled(reason) => self.notify(lformat!("{}: {}", action.title(self), reason)),
			Avail::Hidden => {},
		}
	}

	fn execute(
		&mut self,
		action: Action,
	) {
		match action {
			Palette => self.open_palette(),
			Help => self.open_help(),
			Quit => self.request_quit(),
			Back => self.back(),
			Dashboard => self.go(Screen::Dashboard),
			Projects => self.go(Screen::Projects),
			Log => self.go(Screen::Log),
			TemplateTab => self.go(Screen::Template),
			NextTab => self.switch_screen(true),
			PrevTab => self.switch_screen(false),
			YearPrev => self.change_year(-1),
			YearNext => self.change_year(1),
			Reload if self.screen == Screen::Template => {
				self.template = None;
				self.open_template();
				self.start_loading(None);
			},
			Reload => self.start_loading(None),
			ToggleMouse => self.toggle_mouse(),
			CreateStorage => self.ask_create_storage(),
			Open => self.open_selected(),
			Search if self.screen != Screen::Template => self.start_search(),
			Scope => {
				let next = self.scope.next();
				self.set_scope(next);
			},
			ScopeYear => self.show_scope(Scope::Year),
			ScopeAll => self.show_scope(Scope::AllYears),
			ScopeBroken => self.show_scope(Scope::Broken),
			ToggleArchived => self.toggle_archived(),
			Sort => self.next_sort(),
			ShowActions => self.show_actions(),
			NewProject => self.open_new_project_form(),
			Details => self.open_details_form(),
			OfferEditor => self.open_offer(EditorMode::Offer),
			UsageEditor => self.open_offer(EditorMode::Usage),
			MakeOffer => self.make(BillType::Offer),
			MakeInvoice => self.make(BillType::Invoice),
			Accepted => self.toggle_accepted(),
			InvoiceNumber => self.assign_invoice_number(),
			SetInvoiceDate => self.fill_one("invoice/date"),
			SetPaidDate => self.fill_one("invoice/payed_date"),
			SetWagesDate => self.fill_one("hours/wages_date"),
			MakeMenu => self.open_make_menu(),
			EditFile => self.edit_file(),
			OpenFolder => self.open_folder(),
			Archive => self.archive(),
			Delete => self.delete(),
			More if self.screen == Screen::Template => self.template_step(1.0),
			Less if self.screen == Screen::Template => self.template_step(-1.0),
			More => self.editor_step(1.0),
			Less => self.editor_step(-1.0),
			AsOffered => self.editor_as_offered(),
			NoneUsed => self.editor_none_used(),
			ChangeLine if self.screen == Screen::Template => {
				let row = self.template.as_ref().and_then(|t| t.selected());
				if row.is_some() {
					self.template_product_form(row);
				}
			},
			NewProduct if self.screen == Screen::Template => self.template_product_form(None),
			RemoveLine if self.screen == Screen::Template => self.template_ask_remove(),
			Save if self.screen == Screen::Template => self.save_template(After::Stay),
			Search => self.template_search(),
			TemplatePrev => self.choose_template(-1),
			TemplateNext => self.choose_template(1),
			ClearUsage => self.template_clear_usage(),
			EditTemplateFile => self.edit_template_file(),
			ChangeLine => self.editor_change_line(),
			AddProduct => self.editor_add_product(),
			NewProduct => self.offer_line_form(None),
			RemoveLine => self.editor_remove_line(),
			Save => self.save_offer(),
			ExportCsv => self.ask_export(false),
			ExportIcs => self.ask_export(true),
			GitMenu => self.open_git_menu(),
			GitStatus => self.git(GitOp::Status),
			GitDiff => self.git(GitOp::Diff),
			GitLog => self.git(GitOp::Log),
			GitAdd => self.git(GitOp::AddAll),
			GitCommit => self.git(GitOp::Commit),
			GitPull => self.git(GitOp::Pull),
			GitPush => self.git(GitOp::Push),
		}
	}
}

// ── footer ──────────────────────────────────────────────────────────────────

/// One footer hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
	pub key: String,
	pub label: String,
	pub action: Action,
}

impl Hint {
	pub fn width(&self) -> usize {
		self.key.chars().count() + 1 + self.label.chars().count()
	}
}

/// Space between two hints.
pub const HINT_GAP: usize = 3;

/// The footer for `app`: the available actions by priority, fitted into `width`
/// columns. Help and the command palette always stay, at the end.
pub fn footer(
	app: &App,
	width: usize,
) -> Vec<Hint> {
	let ctx = app.ctx();
	let mut candidates: Vec<(u8, Action)> = Action::all()
		.filter_map(|a| a.footer(ctx).map(|p| (p, a)))
		.filter(|(_, a)| a.availability(app).is_enabled())
		.collect();
	candidates.sort_by_key(|(p, _)| *p);
	let hints = candidates.into_iter().map(|(_, a)| a).collect::<Vec<_>>();
	fit_footer(ctx, &hints, width)
}

/// `hints` in order, as many as fit together with help and commands.
pub fn fit_footer(
	ctx: Ctx,
	actions: &[Action],
	width: usize,
) -> Vec<Hint> {
	let hint = |action: Action| {
		let (key, label) = action.footer_text(ctx);
		Hint { key, label, action }
	};
	let fixed = [hint(Help), hint(Palette)];
	let fixed_width: usize = fixed.iter().map(|h| h.width()).sum::<usize>() + HINT_GAP;
	let mut used = fixed_width;
	let mut shown = Vec::new();
	for &action in actions {
		let h = hint(action);
		if used + h.width() + HINT_GAP > width {
			break;
		}
		used += h.width() + HINT_GAP;
		shown.push(h);
	}
	shown.extend(fixed);
	shown
}

// ── the Actions panel of the project page ───────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelRow {
	Group(String),
	/// the next step of the lifecycle; `None` when there is nothing to do but
	/// wait
	Next {
		action: Option<Action>,
		text: String,
	},
	Item(Action),
}

impl PanelRow {
	pub fn selectable(&self) -> bool {
		!matches!(self, PanelRow::Group(_))
	}

	pub fn action(&self) -> Option<Action> {
		match self {
			PanelRow::Next { action, .. } => *action,
			PanelRow::Item(action) => Some(*action),
			PanelRow::Group(_) => None,
		}
	}
}

const PANEL: [(Group, &[Action]); 4] = [
	(Group::Offer, &[Details, OfferEditor, MakeOffer, Accepted]),
	(
		Group::Invoice,
		&[
			UsageEditor,
			InvoiceNumber,
			SetInvoiceDate,
			MakeInvoice,
			SetPaidDate,
			SetWagesDate,
		],
	),
	(Group::Project, &[Archive, Delete]),
	(Group::Files, &[EditFile, OpenFolder]),
];

/// The rows of the Actions panel for the open project: the next step first,
/// then everything else.
pub fn panel_rows(app: &App) -> Vec<PanelRow> {
	let mut rows = Vec::new();
	if let Some((step, action)) = app.next_step() {
		let text = match (step, action) {
			(_, Some(action)) => action.title(app),
			(Step::Event, None) => lformat!("wait for the catering"),
			(Step::Accepted, None) => lformat!("wait for the client"),
			(Step::Paid, None) => lformat!("wait for the money"),
			(step, None) => step.todo(),
		};
		rows.push(PanelRow::Next { action, text });
	}
	for (group, actions) in PANEL {
		let visible: Vec<Action> = actions
			.iter()
			.copied()
			.filter(|a| a.availability(app) != Avail::Hidden)
			.collect();
		if visible.is_empty() {
			continue;
		}
		rows.push(PanelRow::Group(group.label()));
		rows.extend(visible.into_iter().map(PanelRow::Item));
	}
	rows
}

impl App {
	/// The step to do next on the open project, and the action that does it.
	pub fn next_step(&self) -> Option<(Step, Option<Action>)> {
		self.next_step_of(self.selected_entry()?)
	}

	/// The step to do next on `entry`, and the action that does it.
	pub fn next_step_of(
		&self,
		entry: &super::data::Entry,
	) -> Option<(Step, Option<Action>)> {
		let content = std::fs::read_to_string(entry.project.file()).unwrap_or_default();
		let facts = lifecycle::facts(&entry.project, &content, entry.location, self.today);
		let step = lifecycle::lifecycle(&facts).current?;
		let has_field = |path: &str| {
			super::yaml_edit::fields_in(&content)
				.iter()
				.any(|(p, _, _)| *p == path)
		};
		let action = match step {
			Step::Offer if !facts.offer_missing.is_empty() => Some(Details),
			Step::Offer if facts.counted.1 == 0 => Some(OfferEditor),
			Step::Offer => Some(MakeOffer),
			Step::Accepted => Some(Accepted),
			Step::Event => None,
			Step::Usage => Some(UsageEditor),
			Step::Invoice if facts.invoice_number.is_none() => Some(InvoiceNumber),
			Step::Invoice if facts.invoice_date.is_none() => Some(SetInvoiceDate),
			Step::Invoice if !facts.invoice_missing.is_empty() => Some(Details),
			Step::Invoice => Some(MakeInvoice),
			// a date the file has no field for can only be added in the editor
			Step::Paid if facts.paid.is_none() => Some(if has_field("invoice/payed_date") {
				SetPaidDate
			} else {
				EditFile
			}),
			Step::Paid if facts.staff && facts.wages_paid.is_none() => {
				Some(if has_field("hours/wages_date") {
					SetWagesDate
				} else {
					EditFile
				})
			},
			Step::Paid => None,
			Step::Archived => Some(Archive),
		};
		Some((step, action))
	}

	/// The action a to-do of the dashboard is about.
	pub fn task_action(
		&self,
		task: &asciii::stats::Task,
	) -> Option<Action> {
		match task.kind {
			TaskKind::IssueInvoice => self
				.entry(task.index)
				.and_then(|e| self.next_step_of(e))
				.and_then(|(_, a)| a),
			TaskKind::FollowUp { .. } => Some(SetPaidDate),
			TaskKind::PayEmployees => Some(SetWagesDate),
			TaskKind::Archive => Some(Archive),
			TaskKind::DuplicateInvoiceNumber => Some(Details),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn press(
		code: KeyCode,
		modifiers: KeyModifiers,
	) -> KeyEvent {
		KeyEvent::new(code, modifiers)
	}

	#[test]
	fn no_key_means_two_things_in_one_context() {
		for ctx in Ctx::ALL {
			let mut seen: Vec<(Key, Action)> = Vec::new();
			for action in Action::all().filter(|a| a.in_ctx(ctx)) {
				for &key in action.keys(ctx) {
					if let Some((_, other)) = seen.iter().find(|(k, _)| *k == key) {
						panic!(
							"{:?} in {:?}: {:?} and {:?}",
							key.label(),
							ctx,
							other,
							action
						);
					}
					seen.push((key, action));
				}
			}
		}
	}

	#[test]
	fn every_action_is_listed_once() {
		let all: Vec<Action> = Action::all().collect();
		let unique: std::collections::HashSet<Action> = all.iter().copied().collect();
		assert_eq!(all.len(), unique.len());
		assert_eq!(all.len(), 63);
		// every action exists somewhere, has a label and a description
		for action in Action::all() {
			assert!(
				Ctx::ALL.iter().any(|c| action.in_ctx(*c)),
				"{:?} is nowhere",
				action
			);
			assert!(!action.label().is_empty() && !action.description().is_empty());
		}
	}

	#[test]
	fn keys_are_looked_up_per_context() {
		let key = |c| press(KeyCode::Char(c), KeyModifiers::NONE);
		assert_eq!(for_key(Ctx::Projects, &key('a')), Some(ToggleArchived));
		assert_eq!(for_key(Ctx::Detail, &key('a')), Some(Archive));
		assert_eq!(for_key(Ctx::Offer, &key('a')), Some(AddProduct));
		assert_eq!(for_key(Ctx::Dashboard, &key('q')), Some(Quit));
		// q is back on the project page and in the editor, never quit
		assert_eq!(for_key(Ctx::Detail, &key('q')), Some(Back));
		assert_eq!(for_key(Ctx::Offer, &key('q')), Some(Back));
		assert_eq!(
			for_key(
				Ctx::Offer,
				&press(KeyCode::Char('q'), KeyModifiers::CONTROL)
			),
			Some(Quit)
		);
		assert_eq!(
			for_key(
				Ctx::Detail,
				&press(KeyCode::Char('c'), KeyModifiers::CONTROL)
			),
			Some(Quit)
		);
		assert_eq!(
			for_key(
				Ctx::Offer,
				&press(KeyCode::Char('s'), KeyModifiers::CONTROL)
			),
			Some(Save)
		);
		assert_eq!(for_key(Ctx::Usage, &key('=')), Some(AsOffered));
		assert_eq!(for_key(Ctx::Offer, &key('=')), Some(More));
		// Esc never quits
		for ctx in Ctx::ALL {
			assert_ne!(
				for_key(ctx, &press(KeyCode::Esc, KeyModifiers::NONE)),
				Some(Quit)
			);
		}
		// shifted letters: the terminal may or may not report Shift
		assert_eq!(
			for_key(Ctx::Detail, &press(KeyCode::Char('I'), KeyModifiers::SHIFT)),
			Some(InvoiceNumber)
		);
		assert_eq!(for_key(Ctx::Detail, &key('I')), Some(InvoiceNumber));
		assert_eq!(for_key(Ctx::Detail, &key('i')), None);
		// the palette, VS Code style
		for ctx in Ctx::ALL {
			assert_eq!(
				for_key(ctx, &press(KeyCode::Char('p'), KeyModifiers::CONTROL)),
				Some(Palette)
			);
			assert_eq!(
				for_key(ctx, &press(KeyCode::F(1), KeyModifiers::NONE)),
				Some(Palette)
			);
		}
	}

	#[test]
	fn key_labels() {
		assert_eq!(Key::ch('I').label(), "Shift+I");
		assert_eq!(Key::ctrl('p').label(), "Ctrl+P");
		assert_eq!(Key::code(KeyCode::Enter).label(), "Enter");
		assert_eq!(Key::code(KeyCode::BackTab).label(), "Shift+Tab");
		assert_eq!(Key::ch('?').label(), "?");
	}

	#[test]
	fn footer_keeps_help_and_commands() {
		let actions = [
			Back,
			ShowActions,
			OfferEditor,
			Details,
			MakeMenu,
			InvoiceNumber,
			UsageEditor,
			Accepted,
			Archive,
			EditFile,
		];
		for width in [20, 40, 80, 120, 200] {
			let hints = fit_footer(Ctx::Detail, &actions, width);
			let n = hints.len();
			assert!(n >= 2);
			assert_eq!(hints[n - 2].action, Help);
			assert_eq!(hints[n - 1].action, Palette);
			let total: usize = hints.iter().map(|h| h.width()).sum::<usize>() + HINT_GAP * (n - 1);
			assert!(width < 30 || total <= width, "{} columns: {}", width, total);
			// dropped from the low-priority end
			for (i, hint) in hints[..n - 2].iter().enumerate() {
				assert_eq!(hint.action, actions[i]);
			}
		}
		assert_eq!(
			fit_footer(Ctx::Detail, &actions, 200).len(),
			actions.len() + 2
		);
	}
}
