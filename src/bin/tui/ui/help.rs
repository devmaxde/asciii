//! The help, generated from the action registry: basics, this screen, every
//! other screen.

use ratatui::{
	style::{Color, Style},
	text::{Line, Span},
};

use super::{
	super::{
		actions::{Action, Avail, Ctx},
		app::App,
		data::Urgency,
	},
	dim, key_style, urgency_style, ACCENT,
};

pub const KEY_WIDTH: usize = 18;

fn heading(text: String) -> Line<'static> {
	Line::from(Span::styled(
		text,
		Style::new()
			.fg(ACCENT)
			.add_modifier(ratatui::style::Modifier::BOLD),
	))
}

fn row(
	keys: String,
	text: String,
	style: Style,
) -> Line<'static> {
	let keys = if keys.chars().count() >= KEY_WIDTH {
		format!("{} ", keys)
	} else {
		format!("{:<width$}", keys, width = KEY_WIDTH)
	};
	Line::from(vec![
		Span::raw("  "),
		Span::styled(keys, key_style()),
		Span::styled(text, style),
	])
}

fn keys_of(
	action: Action,
	ctx: Ctx,
) -> String {
	action
		.keys(ctx)
		.iter()
		.map(|k| k.label())
		.collect::<Vec<_>>()
		.join(" ")
}

/// Actions that work the same on every browsing screen; listed once.
fn everywhere(action: Action) -> bool {
	[Ctx::Dashboard, Ctx::Projects, Ctx::Detail, Ctx::Log]
		.iter()
		.all(|c| action.in_ctx(*c))
}

fn section(
	lines: &mut Vec<Line<'static>>,
	ctx: Ctx,
	app: Option<&App>,
) {
	for action in Action::all().filter(|a| a.in_ctx(ctx) && !everywhere(*a)) {
		// what does not apply right now (e.g. creating the folder when it exists) is
		// left out here
		if app.is_some_and(|app| action.availability(app) == Avail::Hidden && action != Action::Back) {
			continue;
		}
		let keys = keys_of(action, ctx);
		let mut text = format!("{} – {}", action.label(), action.description_in(ctx));
		let mut style = Style::new();
		if let Some(Avail::Disabled(reason)) = app.map(|app| action.availability(app)) {
			text = format!("{} – {}", action.label(), reason);
			style = dim();
		}
		let keys = if keys.is_empty() {
			String::from("Ctrl+P")
		} else {
			keys
		};
		lines.push(row(keys, text, style));
	}
}

/// Every line of the help; the current screen comes first, with what is not
/// available right now and why.
pub fn help_lines(app: &App) -> Vec<Line<'static>> {
	let current = app.ctx();
	let mut lines = vec![
        heading(lformat!("Basics")),
        row(String::from("↑ ↓ PgUp PgDn"), lformat!("move in lists, scroll pages (also Home, End and the mouse wheel)"), Style::new()),
        row(String::from("Enter"), lformat!("do the highlighted thing"), Style::new()),
        row(String::from("Esc"), lformat!("one step back; never quits and never drops changes without asking"), Style::new()),
        row(String::from("Ctrl+P  F1  :"), lformat!("all commands and projects, type to search"), Style::new()),
        row(
            lformat!("Mouse"),
            lformat!("click tabs, rows, buttons and hints; Shift-drag (Option-drag in macOS Terminal) selects text"),
            Style::new(),
        ),
        Line::default(),
        heading(lformat!("Here: {}", current.title())),
    ];
	section(&mut lines, current, Some(app));

	lines.push(Line::default());
	lines.push(heading(lformat!("Everywhere")));
	for action in Action::all().filter(|a| everywhere(*a)) {
		// the keys of the project page: `q` is not quit there, it is explained below
		lines.push(row(
			keys_of(action, Ctx::Detail),
			format!("{} – {}", action.label(), action.description()),
			Style::new(),
		));
	}
	lines.push(row(
		String::from("q"),
		lformat!("quits on the tabs; on the project page and in the editor it goes back"),
		Style::new(),
	));

	for ctx in [
		Ctx::Dashboard,
		Ctx::Projects,
		Ctx::Broken,
		Ctx::Detail,
		Ctx::Offer,
		Ctx::Usage,
		Ctx::Template,
		Ctx::Log,
		Ctx::Empty,
	] {
		if ctx == current {
			continue;
		}
		let before = lines.len();
		lines.push(Line::default());
		lines.push(heading(ctx.title()));
		section(&mut lines, ctx, None);
		if lines.len() == before + 2 {
			lines.truncate(before);
		}
	}

	lines.push(Line::default());
	lines.push(heading(lformat!("Colours in the lists")));
	lines.push(Line::from(vec![
		Span::raw("  "),
		Span::styled(lformat!("today"), urgency_style(Urgency::Today)),
		Span::raw("  "),
		Span::styled(lformat!("this week"), urgency_style(Urgency::ThisWeek)),
		Span::raw("  "),
		Span::styled(lformat!("next week"), urgency_style(Urgency::NextWeek)),
		Span::raw("  "),
		Span::styled(lformat!("later"), urgency_style(Urgency::Later)),
		Span::raw("  "),
		Span::styled(lformat!("past 4 weeks"), urgency_style(Urgency::Recent)),
		Span::raw("  "),
		Span::styled(lformat!("older"), urgency_style(Urgency::Old)),
		Span::raw("  "),
		Span::styled(lformat!("archived"), Style::new().fg(Color::DarkGray)),
	]));
	lines.push(Line::from(Span::styled(
        format!("  {}", lformat!("Offer, Invoice: ✓ ready (bold: the PDF exists) · Paid: € client and staff, yellow € partly · Archive: ✓ ready, ▪ archived")),
        dim(),
    )));
	lines
}
