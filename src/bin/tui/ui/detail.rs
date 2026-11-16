//! The project page: a fixed header with the next step, the content, and the
//! Actions panel.

use std::{collections::BTreeMap, fs};

use bill::{Currency, Tax};
use ratatui::{
	layout::{Constraint, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
	Frame,
};

use asciii::{
	project::{
		spec::{HasEmployees, HasEvents, Invoicable, IsClient, IsProject, Offerable, Redeemable},
		Exportable, Project,
	},
	storage::Storable,
};

use super::{
	super::{
		actions::{self, Avail, PanelRow},
		app::App,
		data::Location,
		lifecycle::{self, Lifecycle, Mark, Step},
		mouse::{Hits, Target, Wheel},
		paths, yaml_edit,
	},
	bold, check, dim, dmy, key_style, money, panel, scrollbar, wrap, ACCENT,
};

/// Width of the Actions panel, and the page width from which it is shown beside
/// the content.
pub const PANEL_WIDTH: u16 = 36;
pub const WIDE: u16 = 100;

fn heading(text: String) -> Line<'static> {
	Line::from(Span::styled(
		text,
		Style::new().fg(ACCENT).add_modifier(Modifier::BOLD),
	))
}

fn field(
	label: String,
	value: String,
) -> Line<'static> {
	Line::from(vec![
		Span::styled(format!("{:<17}", label), dim()),
		Span::raw(value),
	])
}

pub fn draw(
	frame: &mut Frame<'_>,
	app: &mut App,
	area: Rect,
	hits: &mut Hits,
) {
	app.wide = area.width >= WIDE;
	let Some(entry) = app.detail.and_then(|i| app.entry(i)) else {
		return;
	};
	let project = &entry.project;
	let content = fs::read_to_string(project.file()).unwrap_or_default();
	let facts = lifecycle::facts(project, &content, entry.location, app.today);
	let cycle = lifecycle::lifecycle(&facts);

	let (page, side) = if app.wide {
		let [page, side] = Layout::horizontal([
			Constraint::Fill(1),
			Constraint::Length(if area.width >= 116 {
				PANEL_WIDTH + 6
			} else {
				PANEL_WIDTH
			}),
		])
		.areas(area);
		(page, Some(side))
	} else {
		(area, None)
	};

	// header: name, where it is, the next step; stays when the content scrolls
	let block = panel(project.short_desc());
	let inner = block.inner(page);
	frame.render_widget(block, page);
	let mut title = vec![Span::styled(project.short_desc(), bold().fg(Color::White))];
	if project.canceled() {
		title.push(Span::styled(
			format!("  {}", lformat!("CANCELED")),
			Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
		));
	}
	let place = match entry.location {
		Location::Working => lformat!("running project"),
		Location::Archive(year) => lformat!("archived in {} (read-only)", year),
	};
	title.push(Span::styled(format!("   {}", place), dim()));
	let mut header = vec![Line::from(title)];
	let (steps, places) = stepper(&cycle, inner.width as usize);
	let steps_at = header.len() as u16;
	header.extend(steps);
	let (next, next_cut) = next_lines(&cycle, app.wide, inner.width as usize);
	header.extend(next);
	let header_height = (header.len() as u16 + 1).min(inner.height);
	let [head, body] =
		Layout::vertical([Constraint::Length(header_height), Constraint::Fill(1)]).areas(inner);
	frame.render_widget(Paragraph::new(header), head);
	// the current step runs its action when clicked
	if let Some(action) = app.next_step().and_then(|(_, action)| action) {
		for (_, row, x, len) in places
			.into_iter()
			.filter(|(step, ..)| Some(*step) == cycle.current)
		{
			let y = head.y + steps_at + row as u16;
			if y < head.bottom() {
				hits.click(Rect::new(head.x + x, y, len, 1), Target::Action(action));
			}
		}
	}
	frame.render_widget(
		Block::new().borders(Borders::TOP).border_style(dim()),
		Rect {
			y: head.bottom().saturating_sub(1),
			height: 1,
			..head
		},
	);

	// content
	let lines = wrap(
		detail_lines(
			project,
			entry.location,
			&entry.summary.duplicate_of,
			&cycle,
			&content,
			body.width,
			next_cut,
		),
		body.width as usize,
	);
	let total = lines.len();
	let visible = body.height as usize;
	let max_scroll = total.saturating_sub(visible) as u16;
	app.detail_scroll = app.detail_scroll.min(max_scroll);
	let scroll = app.detail_scroll;
	frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
	hits.wheel(body, Wheel::Detail);
	scrollbar(
		frame,
		Rect {
			y: body.y.saturating_sub(1),
			height: body.height + 2,
			..page
		},
		total,
		visible,
		scroll as usize,
	);
	if total > visible {
		let position = lformat!(
			" lines {}–{} of {} · PgUp/PgDn ",
			scroll as usize + 1,
			(scroll as usize + visible).min(total),
			total
		);
		let width = position.chars().count() as u16;
		if width + 4 < page.width {
			frame.render_widget(
				Span::styled(position, dim()),
				Rect::new(
					page.right().saturating_sub(width + 2),
					page.bottom().saturating_sub(1),
					width,
					1,
				),
			);
		}
	}

	if let Some(side) = side {
		draw_panel(frame, app, side, hits, false);
	}
}

/// The lifecycle as one left-to-right line: `✓ Offer ─ ✓ Accepted ─ ▶ Invoice ─
/// ○ Paid`. Returns the lines and, per step, its line and columns (for the
/// mouse). It never cuts a step: first the connector gets shorter, then the
/// labels, and only then it wraps.
fn stepper(
	cycle: &Lifecycle,
	width: usize,
) -> (Vec<Line<'static>>, Vec<(Step, usize, u16, u16)>) {
	let current = cycle.steps.iter().position(|v| v.mark == Mark::Current);
	let mark = |m: Mark| match m {
		Mark::Done | Mark::Implied => "✓",
		Mark::Current => "▶",
		Mark::Open => "○",
		Mark::Skipped => "–",
	};
	let text = |short: bool, view: &lifecycle::StepView| {
		format!(
			"{} {}",
			mark(view.mark),
			if short {
				view.step.short_label()
			} else {
				view.step.label()
			}
		)
	};
	let fits = |short: bool, connector: &str| {
		let steps: usize = cycle
			.steps
			.iter()
			.map(|v| text(short, v).chars().count())
			.sum();
		steps + connector.chars().count() * (cycle.steps.len() - 1) <= width
	};
	let options = [
		(false, " ── "),
		(false, " ─ "),
		(false, "─"),
		(true, " ─ "),
		(true, "─"),
	];
	let (short, connector, rows) = options
		.iter()
		.find(|(short, connector)| fits(*short, connector))
		.map(|(s, c)| (*s, *c, 1))
		.unwrap_or((true, " ─ ", 2));
	let per_row = cycle.steps.len().div_ceil(rows);

	let mut lines = Vec::new();
	let mut places = Vec::new();
	for (row, chunk) in cycle.steps.chunks(per_row).enumerate() {
		let mut spans = Vec::new();
		let mut x = 0u16;
		for (n, view) in chunk.iter().enumerate() {
			let index = row * per_row + n;
			if n > 0 || row > 0 {
				// the way up to the current step is done
				let done = current.is_none_or(|c| index <= c);
				let style = if done {
					Style::new().fg(Color::Green)
				} else {
					dim()
				};
				let connector = if n == 0 {
					connector.trim_start().to_owned()
				} else {
					connector.to_owned()
				};
				x += connector.chars().count() as u16;
				spans.push(Span::styled(connector, style));
			}
			let style = match view.mark {
				Mark::Done => Style::new().fg(Color::Green),
				Mark::Implied => Style::new().fg(Color::Green).add_modifier(Modifier::DIM),
				Mark::Current => bold().fg(ACCENT),
				Mark::Open | Mark::Skipped => dim(),
			};
			let label = text(short, view);
			let len = label.chars().count() as u16;
			places.push((view.step, row, x, len));
			x += len;
			spans.push(Span::styled(label, style));
		}
		lines.push(Line::from(spans));
	}
	(lines, places)
}

/// What the current step needs, in at most two lines, or that the project is
/// done.
fn next_lines(
	cycle: &Lifecycle,
	wide: bool,
	width: usize,
) -> (Vec<Line<'static>>, bool) {
	let line = match (cycle.current_view(), cycle.canceled) {
		(None, _) => Line::from(Span::styled(
			lformat!("Done: the project is archived."),
			Style::new().fg(Color::Green),
		)),
		(Some(view), canceled) => {
			let mut spans = vec![
				Span::styled(format!("{} ", lformat!("Next:")), bold()),
				Span::styled(view.step.todo(), bold().fg(ACCENT)),
			];
			if canceled {
				spans.push(Span::styled(
					format!("  ({})", lformat!("canceled")),
					Style::new().fg(Color::Red),
				));
			}
			if let Some(todo) = &view.todo {
				spans.push(Span::raw(format!(" → {}", todo)));
			} else if !view.text.is_empty() {
				spans.push(Span::styled(format!(" · {}", view.text), dim()));
			}
			spans.push(Span::styled(
				format!(
					"  {}",
					if wide {
						lformat!("(Enter runs it, see Actions)")
					} else {
						lformat!("(Enter: actions)")
					}
				),
				dim(),
			));
			Line::from(spans)
		},
	};
	let mut lines = wrap(vec![line], width);
	let cut = lines.len() > 2;
	lines.truncate(2);
	if cut {
		// the second line ends with "…"; the whole text is in the Fields section
		if let Some(last) = lines.last_mut() {
			let mut text: String = last.spans.iter().map(|s| s.content.as_ref()).collect();
			let style = last.spans.last().map(|s| s.style).unwrap_or_default();
			while text.chars().count() + 1 > width && text.pop().is_some() {}
			let kept = text.trim_end_matches([' ', ';', ',', '→', '·']).to_owned();
			// keep the styles of the spans that still fit, the rest is cut
			let mut spans = Vec::new();
			let mut left = kept.chars().count();
			for span in &last.spans {
				if left == 0 {
					break;
				}
				let part: String = span.content.chars().take(left).collect();
				left -= part.chars().count();
				spans.push(Span::styled(part, span.style));
			}
			spans.push(Span::styled("…", style));
			*last = Line::from(spans);
		}
	}
	(lines, cut)
}

/// The Actions panel: the next step first, then the groups; disabled rows say
/// why below.
pub fn draw_panel(
	frame: &mut Frame<'_>,
	app: &mut App,
	area: Rect,
	hits: &mut Hits,
	popup: bool,
) {
	let rows = actions::panel_rows(app);
	if rows.is_empty() {
		return;
	}
	if !rows.get(app.panel).is_some_and(PanelRow::selectable) {
		app.panel = rows.iter().position(PanelRow::selectable).unwrap_or(0);
	}
	let title = if popup {
		lformat!("Actions · Enter runs · Esc closes")
	} else {
		lformat!("Actions")
	};
	let block =
		panel(title).border_style(Style::new().fg(if popup { ACCENT } else { Color::DarkGray }));
	let inner = block.inner(area);
	frame.render_widget(block, area);
	let [list_area, about] =
		Layout::vertical([Constraint::Fill(1), Constraint::Length(4)]).areas(inner);
	let width = list_area.width as usize;

	let items: Vec<ListItem<'_>> = rows
		.iter()
		.map(|row| match row {
			PanelRow::Group(name) => ListItem::new(Line::from(Span::styled(
				name.clone(),
				dim().add_modifier(Modifier::BOLD),
			))),
			PanelRow::Next { action, text } => {
				let key = action
					.and_then(|a| {
						a.keys(super::super::actions::Ctx::Detail)
							.first()
							.map(|k| k.label())
					})
					.unwrap_or_default();
				let label = format!("▶ {} {}", lformat!("Next:"), text);
				ListItem::new(fill(label, key, width, bold().fg(ACCENT)))
			},
			PanelRow::Item(action) => {
				let key = action
					.keys(super::super::actions::Ctx::Detail)
					.first()
					.map(|k| k.label())
					.unwrap_or_default();
				let enabled = action.availability(app).is_enabled();
				let style = if enabled { Style::new() } else { dim() };
				ListItem::new(fill(format!("  {}", action.title(app)), key, width, style))
			},
		})
		.collect();
	let mut state = ListState::default().with_selected(Some(app.panel));
	let highlight = if popup || app.modal.is_none() {
		Style::new().fg(Color::Black).bg(ACCENT)
	} else {
		Style::new().add_modifier(Modifier::REVERSED)
	};
	frame.render_stateful_widget(
		List::new(items).highlight_style(highlight),
		list_area,
		&mut state,
	);
	let offset = state.offset();
	for n in 0..rows
		.len()
		.saturating_sub(offset)
		.min(list_area.height as usize)
	{
		hits.click(
			Rect::new(list_area.x, list_area.y + n as u16, list_area.width, 1),
			Target::PanelRow(offset + n),
		);
	}
	hits.wheel(list_area, Wheel::Panel);

	// what the highlighted action does, or why it is not available
	let selected = rows.get(app.panel);
	let text = match selected.and_then(PanelRow::action) {
		Some(action) => match action.availability(app) {
			Avail::Disabled(reason) => Line::from(vec![
				Span::styled(
					format!("{} ", lformat!("Not available:")),
					Style::new().fg(Color::Yellow),
				),
				Span::raw(reason),
			]),
			_ => Line::from(Span::styled(action.description(), dim())),
		},
		None => Line::from(Span::styled(
			lformat!("Nothing to do right now; the project waits for the next step."),
			dim(),
		)),
	};
	frame.render_widget(
		Paragraph::new(text)
			.wrap(Wrap { trim: true })
			.block(Block::new().borders(Borders::TOP).border_style(dim())),
		about,
	);
}

/// `label` left, `key` right, in `width` columns.
fn fill(
	label: String,
	key: String,
	width: usize,
	style: Style,
) -> Line<'static> {
	let key_width = key.chars().count();
	let room = width.saturating_sub(key_width + 1);
	let label = super::truncate(&label, room);
	let gap = width.saturating_sub(label.chars().count() + key_width);
	Line::from(vec![
		Span::styled(label, style),
		Span::raw(" ".repeat(gap)),
		Span::styled(key, key_style()),
	])
}

/// The lifecycle: one line per step, the current one highlighted.
/// `Sat 12.12.2026`, the weekday in the language of the UI.
fn day<D: chrono::Datelike + Copy>(date: D) -> String {
	use chrono::Weekday::*;
	let name = match date.weekday() {
		Mon => lformat!("Mon"),
		Tue => lformat!("Tue"),
		Wed => lformat!("Wed"),
		Thu => lformat!("Thu"),
		Fri => lformat!("Fri"),
		Sat => lformat!("Sat"),
		Sun => lformat!("Sun"),
	};
	format!(
		"{} {:02}.{:02}.{}",
		name,
		date.day(),
		date.month(),
		date.year()
	)
}

fn detail_lines(
	project: &Project,
	location: Location,
	duplicate_of: &[String],
	cycle: &Lifecycle,
	content: &str,
	width: u16,
	next_cut: bool,
) -> Vec<Line<'static>> {
	let mut lines = Vec::new();
	// what the lifecycle knows that no section below shows
	let step_text = |step: Step| {
		cycle
			.steps
			.iter()
			.find(|v| v.step == step)
			.map(|v| v.text.clone())
			.unwrap_or_default()
	};
	let e_invoice = lifecycle::e_invoice(project).filter(|_| location == Location::Working);

	// the next step did not fit into the header: all of it first
	if let Some(todo) = cycle
		.current_view()
		.and_then(|v| v.todo.clone())
		.filter(|_| next_cut)
	{
		lines.push(Line::from(vec![
			Span::styled("▶ ", Style::new().fg(ACCENT)),
			Span::raw(lformat!("next step: {}", todo)),
		]));
		lines.push(Line::default());
	}

	// event
	lines.push(heading(lformat!("Event")));
	if let Some(events) = project.events() {
		for event in events {
			let mut text = day(event.begin);
			if let Some(end) = event.end {
				text.push_str(&format!(" – {}", day(end)));
			}
			let times: Vec<String> = event
				.times
				.iter()
				.map(|t| {
					if t.start == t.end {
						t.start.format("%H:%M").to_string()
					} else {
						format!("{}–{}", t.start.format("%H:%M"), t.end.format("%H:%M"))
					}
				})
				.collect();
			if !times.is_empty() {
				text.push_str(&format!("  {}", times.join(", ")));
			}
			lines.push(field(lformat!("Date"), text));
		}
		// "(in 12 days)", "(today)"
		if let Some((_, when)) = step_text(Step::Event).split_once("  (") {
			lines.push(field(String::new(), format!("({}", when)));
		}
	} else {
		lines.push(field(lformat!("Date"), dmy(project.event_date().ok())));
	}
	if let Ok(location) = project.location() {
		lines.push(field(lformat!("Location"), location.to_owned()));
	}
	lines.push(field(
		lformat!("Manager"),
		project.responsible().unwrap_or("").to_owned(),
	));
	lines.push(Line::default());

	// client
	let client = project.client();
	lines.push(heading(lformat!("Client")));
	let name = [
		client.title().ok(),
		client.first_name().ok(),
		client.last_name().ok(),
	]
	.iter()
	.flatten()
	.copied()
	.collect::<Vec<_>>()
	.join(" ");
	lines.push(field(
		lformat!("Name"),
		if name.is_empty() {
			lformat!("not filled in (p)")
		} else {
			name
		},
	));
	if let Ok(email) = client.email() {
		lines.push(field(lformat!("Email"), email.to_owned()));
	}
	if let Ok(address) = client.address() {
		for (n, line) in address.lines().enumerate() {
			let label = if n == 0 {
				lformat!("Address")
			} else {
				String::new()
			};
			lines.push(field(label, line.to_owned()));
		}
	}
	if let Some(addressing) = client.addressing() {
		lines.push(field(lformat!("Salutation"), addressing));
	}
	lines.push(Line::default());

	// offer / invoice
	lines.push(heading(lformat!("Offer and invoice")));
	lines.push(field(
		lformat!("Offer"),
		format!(
			"{}  {}",
			project.offer().number().unwrap_or_default(),
			dmy(project.offer().date().ok())
		),
	));
	lines.push(field(lformat!("Accepted"), step_text(Step::Accepted)));
	lines.push(field(lformat!("Actual usage"), step_text(Step::Usage)));
	let mut invoice = format!(
		"{}  {}",
		project.invoice().number_long_str().unwrap_or_default(),
		dmy(project.invoice().date().ok())
	);
	if let Ok(official) = project.invoice().official() {
		invoice.push_str(&format!("  ({})", official));
	}
	lines.push(field(lformat!("Invoice"), invoice));
	if !duplicate_of.is_empty() {
		lines.push(Line::from(Span::styled(
			lformat!(
				"⚠ This invoice number is also used by {}. Invoice numbers must be unique per year.",
				duplicate_of.join(", ")
			),
			Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
		)));
	}
	lines.push(field(
		lformat!("Paid on"),
		project
			.payed_date()
			.ok()
			.map(|d| dmy(Some(d)))
			.unwrap_or_else(|| lformat!("not yet")),
	));
	lines.push(Line::default());

	lines.extend(bill_lines(project, width));
	lines.push(Line::default());

	// staff
	let hours = project.hours();
	lines.push(heading(lformat!("Staff")));
	match hours.employees() {
		Ok(employees) if !employees.is_empty() => {
			for e in employees {
				lines.push(Line::from(format!(
					"  {:<24} {:>6} h × {:>9} = {:>11}",
					super::truncate(&e.name, 24),
					e.time,
					money(e.salary),
					money(e.wage)
				)));
			}
			lines.push(field(
				lformat!("Wages paid"),
				hours
					.wages_date()
					.ok()
					.map(|d| dmy(Some(d)))
					.unwrap_or_else(|| lformat!("not yet")),
			));
		},
		_ => lines.push(Line::from(Span::styled(lformat!("  no staff"), dim()))),
	}
	lines.push(Line::default());

	// fields and placeholders, read from the YAML structure
	lines.push(heading(lformat!("Fields")));
	let empty: Vec<String> = yaml_edit::fields_in(content)
		.into_iter()
		.filter(|(_, _, value)| value.is_none())
		.map(|(path, _, _)| yaml_edit::label(path))
		.collect();
	if !empty.is_empty() {
		lines.push(Line::from(vec![
			Span::styled("· ", Style::new().fg(Color::Yellow)),
			Span::raw(lformat!(
				"still empty: {} (Project details, p)",
				empty.join(", ")
			)),
		]));
	}
	let leftovers = yaml_edit::leftover_placeholders(content);
	if !leftovers.is_empty() {
		lines.push(Line::from(vec![
			Span::styled(
				"⚠ ",
				Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
			),
			Span::raw(lformat!(
				"placeholders inside values, they would be printed as they are (Edit file, e): {}",
				leftovers
					.iter()
					.map(|w| format!("##{}##", w))
					.collect::<Vec<_>>()
					.join(", ")
			)),
		]));
	}
	if empty.is_empty() && leftovers.is_empty() {
		lines.push(Line::from(vec![
			Span::styled("✓ ", Style::new().fg(Color::Green)),
			Span::raw(lformat!("all fields filled in")),
		]));
	}
	lines.push(Line::default());

	// ZUGFeRD, only when switched on (document_export/zugferd)
	if let Some(needs) = e_invoice {
		lines.push(heading(lformat!("E-invoice (ZUGFeRD)")));
		if needs.is_empty() {
			lines.push(Line::from(vec![
				Span::styled("✓ ", Style::new().fg(Color::Green)),
				Span::raw(lformat!("complete, it is attached to the invoice PDF")),
			]));
		} else {
			for need in needs {
				lines.push(Line::from(vec![
					Span::styled("· ", Style::new().fg(Color::Yellow)),
					Span::raw(need),
				]));
			}
		}
		if let Some(xml) = project.invoice_file().map(|pdf| pdf.with_extension("xml")) {
			lines.push(Line::from(vec![
				Span::styled(format!("{:<17}", "XML"), dim()),
				check(xml.exists(), false),
				Span::raw(format!(" {}", paths::short(&xml))),
			]));
		}
		lines.push(Line::default());
	}

	// files
	lines.push(heading(lformat!("Files")));
	lines.push(field(lformat!("Project"), paths::short(&project.file())));
	for (label, path) in [
		(lformat!("Offer PDF"), project.offer_file()),
		(lformat!("Invoice PDF"), project.invoice_file()),
	] {
		if let Some(path) = path {
			let exists = path.exists();
			lines.push(Line::from(vec![
				Span::styled(format!("{:<17}", label), dim()),
				check(exists, false),
				Span::raw(format!(" {}", paths::short(&path))),
				Span::styled(
					if exists {
						String::new()
					} else {
						format!("  {}", lformat!("(not created yet)"))
					},
					dim(),
				),
			]));
		}
	}
	if let Ok(entries) = fs::read_dir(project.dir()) {
		let mut names: Vec<String> = entries
			.filter_map(Result::ok)
			.map(|e| e.file_name().to_string_lossy().into_owned())
			.collect();
		names.sort();
		lines.push(field(lformat!("In folder"), names.join(", ")));
	}
	lines
}

/// Offer and invoice side by side; the item column takes what the width leaves.
fn bill_lines(
	project: &Project,
	width: u16,
) -> Vec<Line<'static>> {
	let mut lines = vec![heading(lformat!("Bill"))];
	let (offer, invoice) = match project.bills() {
		Ok(bills) => bills,
		Err(error) => {
			lines.push(Line::from(Span::styled(
				lformat!("  cannot compute the bill: {}", error),
				Style::new().fg(Color::Red),
			)));
			return lines;
		},
	};
	// the invoice is what was actually sold (amount minus returned); match its
	// items to the offer
	struct BillRow {
		key: String,
		label: String,
		price: Currency,
		offered: Option<f64>,
		offer_cost: Option<Currency>,
		sold: Option<f64>,
		invoice_cost: Option<Currency>,
	}
	let label = |name: &str, unit: Option<&str>| match unit {
		Some(unit) => format!("{} ({})", name, unit),
		None => name.to_owned(),
	};
	let mut rows: Vec<BillRow> = Vec::new();
	for item in offer.as_items() {
		let p = &item.product;
		rows.push(BillRow {
			key: format!("{}|{:?}|{}", p.name, p.unit, p.price.value()),
			label: label(p.name, p.unit),
			price: p.price,
			offered: Some(item.amount),
			offer_cost: Some(item.gross()),
			sold: None,
			invoice_cost: None,
		});
	}
	for item in invoice.as_items() {
		let p = &item.product;
		let key = format!("{}|{:?}|{}", p.name, p.unit, p.price.value());
		match rows
			.iter_mut()
			.find(|row| row.key == key && row.sold.is_none())
		{
			Some(row) => {
				row.sold = Some(item.amount);
				row.invoice_cost = Some(item.gross());
			},
			None => rows.push(BillRow {
				key,
				label: label(p.name, p.unit),
				price: p.price,
				offered: None,
				offer_cost: None,
				sold: Some(item.amount),
				invoice_cost: Some(item.gross()),
			}),
		}
	}

	// # item price │ offered offer │ used invoice
	// the amount columns are as wide as their headings (German ones are longer)
	let qty = [lformat!("Offered"), lformat!("Used")]
		.iter()
		.map(|s| s.chars().count())
		.max()
		.unwrap_or(7)
		.clamp(7, 10);
	let fixed: usize = 2 + 2 + 2 + 1 + 9 + 3 + qty + 1 + 10 + 3 + qty + 1 + 10;
	let item = (width as usize).saturating_sub(fixed + 1).clamp(8, 30);
	let amount = |a: Option<f64>| {
		a.map(|a| a.to_string())
			.unwrap_or_else(|| String::from("–"))
	};
	let cost = |c: Option<Currency>| c.map(money).unwrap_or_else(|| String::from("–"));
	let sums_line = |label: String, offer: String, invoice: String| {
		format!(
			"  {:<w$}{:>10} │ {:>qty$} {:>10}",
			super::truncate(&label, item + 2 + 2 + 1 + 9 + 3 + qty + 1),
			offer,
			"",
			invoice,
			w = item + 2 + 2 + 1 + 9 + 3 + qty + 1,
			qty = qty
		)
	};

	lines.push(Line::from(Span::styled(
		format!(
			"  {:>2}  {:<item$} {:>9} │ {:>qty$} {:>10} │ {:>qty$} {:>10}",
			"#",
			super::truncate(&lformat!("Item"), item),
			super::truncate(&lformat!("Price"), 9),
			super::truncate(&lformat!("Offered"), qty),
			super::truncate(&lformat!("Offer"), 10),
			super::truncate(&lformat!("Used"), qty),
			super::truncate(&lformat!("Invoice"), 10),
			item = item,
			qty = qty
		),
		dim(),
	)));
	for (n, row) in rows.iter().enumerate() {
		let changed = row.offered != row.sold;
		let invoice_part = format!("{:>qty$} {:>10}", amount(row.sold), cost(row.invoice_cost));
		lines.push(Line::from(vec![
			Span::raw(format!(
				"  {:>2}  {:<item$} {:>9} │ {:>qty$} {:>10} │ ",
				n + 1,
				super::truncate(&row.label, item),
				money(row.price),
				amount(row.offered),
				cost(row.offer_cost),
				item = item,
				qty = qty
			)),
			// items where the sold amount differs from the offer stand out
			if changed {
				Span::styled(invoice_part, Style::new().fg(Color::Yellow))
			} else {
				Span::raw(invoice_part)
			},
		]));
	}
	lines.push(Line::from(Span::styled(
		format!("  {}", "─".repeat(fixed - 2 + item)),
		dim(),
	)));

	// VAT groups of both bills
	let mut groups: BTreeMap<Tax, [Option<(Currency, Currency)>; 2]> = BTreeMap::new();
	for (tax, items) in offer.iter() {
		groups.entry(*tax).or_default()[0] = Some((items.gross_sum(), items.tax_sum()));
	}
	for (tax, items) in invoice.iter() {
		groups.entry(*tax).or_default()[1] = Some((items.gross_sum(), items.tax_sum()));
	}
	for (tax, [o, i]) in &groups {
		let rate = tax.value() * 100.0;
		lines.push(Line::from(sums_line(
			lformat!("sum at {}% VAT", rate),
			cost(o.map(|(gross, _)| gross)),
			cost(i.map(|(gross, _)| gross)),
		)));
		let has_tax = [o, i]
			.iter()
			.any(|s| s.is_some_and(|(_, tax)| tax.value() > 0));
		if has_tax {
			lines.push(Line::from(sums_line(
				lformat!("+ {}% VAT", rate),
				cost(o.map(|(_, tax)| tax)),
				cost(i.map(|(_, tax)| tax)),
			)));
		}
	}
	lines.push(Line::from(Span::styled(
		sums_line(
			lformat!("Total"),
			money(offer.net_total()),
			money(invoice.net_total()),
		),
		bold(),
	)));
	lines
}
