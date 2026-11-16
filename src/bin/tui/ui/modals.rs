//! Popups. Each registers its area first, so clicks outside it do not reach the
//! screen below.

use ratatui::{
	layout::{Constraint, Layout, Rect},
	style::{Color, Modifier, Style},
	text::{Line, Span},
	widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
	Frame,
};

use super::{
	super::{
		app::App,
		dialogs::{Dialog, Modal, Pick, Tone, Typed},
		forms::{FieldKind, Form},
		mouse::{Hits, Target, Wheel},
		palette::{self, Palette},
	},
	bold, button_row, button_row_width, centered, dim, help_lines, key_style, panel, scrollbar,
	truncate, wrap, ACCENT,
};

/// Footer hints while a popup is open.
pub fn footer(modal: &Modal) -> Vec<(String, String)> {
	let pair = |k: &str, l: String| (k.to_owned(), l);
	match modal {
		Modal::Help { .. } | Modal::Output { .. } => vec![
			pair("↑↓ PgUp PgDn", lformat!("scroll")),
			pair("Esc", lformat!("close")),
		],
		Modal::Palette(_) => vec![
			pair("↑↓", lformat!("choose")),
			pair("Enter", lformat!("run")),
			pair("Esc", lformat!("close")),
		],
		Modal::Dialog(_) => vec![
			pair("←→ Tab", lformat!("choose")),
			pair("Enter", lformat!("press")),
			pair("Esc", lformat!("cancel")),
		],
		Modal::Typed(_) => vec![
			pair("Enter", lformat!("delete")),
			pair("Tab", lformat!("buttons")),
			pair("Esc", lformat!("cancel")),
		],
		Modal::Form(_) => vec![
			pair("Tab ↓", lformat!("next")),
			pair("Shift+Tab ↑", lformat!("previous")),
			pair("Enter Ctrl+S", lformat!("save")),
			pair("Esc", lformat!("cancel")),
		],
		Modal::Pick(_) | Modal::Actions => vec![
			pair("↑↓", lformat!("choose")),
			pair("Enter", lformat!("run")),
			pair("Esc", lformat!("close")),
		],
		Modal::CatalogSync { .. } => vec![
			pair("Space", lformat!("choose")),
			pair("Enter", lformat!("write chosen")),
			pair("Esc", lformat!("keep the template")),
		],
	}
}

fn frame_block(
	title: String,
	tone: Tone,
) -> Block<'static> {
	let color = match tone {
		Tone::Info => ACCENT,
		Tone::Warning => Color::Yellow,
		Tone::Danger => Color::Red,
		Tone::Success => Color::Green,
	};
	let mark = match tone {
		Tone::Warning | Tone::Danger => "⚠ ",
		Tone::Success => "✓ ",
		Tone::Info => "",
	};
	Block::bordered()
		.border_type(BorderType::Rounded)
		.border_style(Style::new().fg(color))
		.title(Span::styled(
			format!(" {}{} ", mark, title),
			bold().fg(color),
		))
}

pub fn draw(
	frame: &mut Frame<'_>,
	app: &mut App,
	area: Rect,
	hits: &mut Hits,
) {
	let Some(mut modal) = app.modal.take() else {
		return;
	};
	match &mut modal {
		Modal::Help { scroll } => draw_help(frame, app, area, scroll, hits),
		Modal::Palette(palette) => draw_palette(frame, app, area, palette, hits),
		Modal::Dialog(dialog) => draw_dialog(frame, area, dialog, hits),
		Modal::Typed(typed) => draw_typed(frame, area, typed, hits),
		Modal::Form(form) => draw_form(frame, area, form, hits),
		Modal::Pick(pick) => draw_pick(frame, area, pick, hits),
		Modal::Output {
			title,
			text,
			scroll,
		} => draw_output(frame, area, title, text, scroll, hits),
		Modal::CatalogSync {
			template,
			changes,
			state,
			focus,
		} => {
			let name = template
				.file_stem()
				.map(|n| n.to_string_lossy().into_owned())
				.unwrap_or_default();
			let rect = centered(area, 84, changes.len() as u16 + 9);
			open(frame, rect, hits);
			let block = frame_block(lformat!("Update the template \"{}\"?", name), Tone::Warning);
			let inner = block.inner(rect);
			frame.render_widget(block, rect);
			let [intro, list_area, buttons] = Layout::vertical([
				Constraint::Length(3),
				Constraint::Fill(1),
				Constraint::Length(2),
			])
			.areas(inner);
			frame.render_widget(
                Paragraph::new(lformat!(
                    "The catalog of this project changed. Should new projects get these products and prices too? Choose no for special prices of this customer."
                ))
                .wrap(Wrap { trim: false }),
                intro,
            );
			let items = changes.iter().map(|(change, chosen)| {
				let mark = if *chosen {
					Span::styled("[x] ", Style::new().fg(Color::Green))
				} else {
					Span::styled("[ ] ", dim())
				};
				ListItem::new(Line::from(vec![mark, Span::raw(change.describe())]))
			});
			let list = List::new(items)
				.highlight_style(if *focus == 0 {
					Style::new().add_modifier(Modifier::REVERSED)
				} else {
					Style::new()
				})
				.highlight_symbol("▶ ");
			frame.render_stateful_widget(list, list_area, state);
			let offset = state.offset();
			for n in 0..changes
				.len()
				.saturating_sub(offset)
				.min(list_area.height as usize)
			{
				hits.click(
					Rect::new(list_area.x, list_area.y + n as u16, list_area.width, 1),
					Target::CatalogRow(offset + n),
				);
			}
			let labels = [
				lformat!("Write chosen to the template"),
				lformat!("Keep the template"),
			];
			let row = Rect {
				y: buttons.y + 1,
				height: 1,
				..buttons
			};
			let focused = (*focus > 0).then(|| *focus - 1);
			for (n, rect) in button_row(
				frame,
				row,
				&labels,
				focused.or(Some(0)).filter(|_| *focus > 0),
				None,
			)
			.into_iter()
			.enumerate()
			{
				hits.click(rect, Target::CatalogButton(n + 1));
			}
		},
		Modal::Actions => {
			let rows = super::super::actions::panel_rows(app).len() as u16;
			// wider than the side panel, so long (German) labels fit
			let rect = centered(
				area,
				area
					.width
					.saturating_sub(8)
					.clamp(super::detail::PANEL_WIDTH + 6, 64),
				rows + 8,
			);
			open(frame, rect, hits);
			super::detail::draw_panel(frame, app, rect, hits, true);
		},
	}
	app.modal = Some(modal);
}

/// Clears `rect` and makes it the popup area for the mouse.
fn open(
	frame: &mut Frame<'_>,
	rect: Rect,
	hits: &mut Hits,
) {
	frame.render_widget(Clear, rect);
	hits.start_modal(rect);
}

fn draw_help(
	frame: &mut Frame<'_>,
	app: &App,
	area: Rect,
	scroll: &mut u16,
	hits: &mut Hits,
) {
	let width = area.width.saturating_sub(4).min(110);
	let rect = centered(area, width, area.height);
	open(frame, rect, hits);
	let lines = super::wrap_hanging(
		help_lines(app),
		width.saturating_sub(3) as usize,
		2 + super::help::KEY_WIDTH,
	);
	let visible = rect.height.saturating_sub(2) as usize;
	let total = lines.len();
	*scroll = (*scroll).min(total.saturating_sub(visible) as u16);
	let title = if total > visible {
		lformat!(
			"Help · lines {}–{} of {} · ↑↓ PgUp PgDn · Esc closes",
			*scroll as usize + 1,
			(*scroll as usize + visible).min(total),
			total
		)
	} else {
		lformat!("Help · Esc closes")
	};
	frame.render_widget(
		Paragraph::new(lines)
			.block(panel(title).border_style(Style::new().fg(ACCENT)))
			.scroll((*scroll, 0)),
		rect,
	);
	hits.wheel(rect, Wheel::Help);
	scrollbar(frame, rect, total, visible, *scroll as usize);
}

fn draw_palette(
	frame: &mut Frame<'_>,
	app: &App,
	area: Rect,
	palette: &mut Palette,
	hits: &mut Hits,
) {
	let rows = palette::rows(app, palette.input.text());
	palette.selected = palette.selected.min(rows.len().saturating_sub(1));
	let width = area.width.saturating_sub(4).min(90);
	let height = (rows.len() as u16 + 5)
		.clamp(8, area.height.saturating_sub(2).max(8))
		.min(area.height);
	let rect = Rect {
		y: area.y + 1.min(area.height.saturating_sub(height)),
		..centered(area, width, height)
	};
	open(frame, rect, hits);
	let block = panel(lformat!("Commands and projects")).border_style(Style::new().fg(ACCENT));
	let inner = block.inner(rect);
	frame.render_widget(block, rect);
	let [input, line, list_area, hint] = Layout::vertical([
		Constraint::Length(1),
		Constraint::Length(1),
		Constraint::Fill(1),
		Constraint::Length(1),
	])
	.areas(inner);

	let (before, at, after) = palette
		.input
		.visible(inner.width.saturating_sub(4) as usize);
	let mut spans = vec![
		Span::styled("› ", key_style()),
		Span::raw(before),
		Span::styled(at, Style::new().fg(Color::Black).bg(Color::White)),
		Span::raw(after),
	];
	if palette.input.is_empty() {
		spans.push(Span::styled(
			lformat!(" type to search commands and projects"),
			dim(),
		));
	}
	frame.render_widget(Line::from(spans), input);
	frame.render_widget(Block::new().borders(Borders::TOP).border_style(dim()), line);

	let width = list_area.width as usize;
	let items: Vec<ListItem<'_>> = rows
		.iter()
		.map(|row| {
			let keys = row.keys.clone();
			let room = width.saturating_sub(keys.chars().count() + 2);
			let (text, style) = match &row.disabled {
				Some(reason) => (format!("{} – {}", row.label, reason), dim()),
				None => (row.label.clone(), Style::new()),
			};
			let text = truncate(&text, room);
			let gap = width.saturating_sub(text.chars().count() + keys.chars().count() + 1);
			ListItem::new(Line::from(vec![
				Span::styled(format!(" {}", text), style),
				Span::raw(" ".repeat(gap)),
				Span::styled(
					keys,
					if row.disabled.is_some() {
						dim()
					} else {
						key_style()
					},
				),
			]))
		})
		.collect();
	if items.is_empty() {
		frame.render_widget(
			Paragraph::new(lformat!(" nothing matches")).style(dim()),
			list_area,
		);
	}
	let mut state = ListState::default().with_selected(Some(palette.selected));
	frame.render_stateful_widget(
		List::new(items).highlight_style(Style::new().fg(Color::Black).bg(ACCENT)),
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
			Target::PaletteRow(offset + n),
		);
	}
	hits.wheel(list_area, Wheel::Palette);
	frame.render_widget(
		Line::from(Span::styled(
			lformat!("Enter runs · Esc closes · greyed out: not available here, with the reason"),
			dim(),
		)),
		hint,
	);
}

/// Height of `lines` wrapped to `width`.
fn wrapped_height(
	lines: &[String],
	width: u16,
) -> u16 {
	lines
		.iter()
		.map(|l| {
			wrap(vec![Line::from(l.clone())], width as usize)
				.len()
				.max(1) as u16
		})
		.sum()
}

fn draw_dialog(
	frame: &mut Frame<'_>,
	area: Rect,
	dialog: &mut Dialog,
	hits: &mut Hits,
) {
	let labels: Vec<String> = dialog.buttons.iter().map(|b| b.label.clone()).collect();
	let longest = dialog
		.lines
		.iter()
		.map(|l| l.chars().count() as u16)
		.max()
		.unwrap_or(0);
	let width = (longest + 4)
		.max(button_row_width(&labels) + 4)
		.max(dialog.title.chars().count() as u16 + 8)
		.clamp(40, 90);
	let width = width.min(area.width.saturating_sub(2));
	let text_height = wrapped_height(&dialog.lines, width.saturating_sub(2));
	let rect = centered(area, width, text_height + 4);
	open(frame, rect, hits);
	let block = frame_block(dialog.title.clone(), dialog.tone);
	let inner = block.inner(rect);
	frame.render_widget(block, rect);
	let [text, buttons] = Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(inner);
	let lines: Vec<Line<'static>> = wrap(
		dialog.lines.iter().map(|l| Line::from(l.clone())).collect(),
		text.width as usize,
	);
	frame.render_widget(Paragraph::new(lines), text);
	let row = Rect {
		y: buttons.y + 1,
		height: 1,
		..buttons
	};
	let danger = (dialog.tone == Tone::Danger).then_some(0);
	for (n, rect) in button_row(frame, row, &labels, Some(dialog.focus), danger)
		.into_iter()
		.enumerate()
	{
		hits.click(rect, Target::Button(n));
	}
}

fn input_spans(
	text: &super::super::text_input::TextInput,
	width: usize,
	focused: bool,
) -> Vec<Span<'static>> {
	// the focused field is lighter and underlined, so it stands out even when
	// colours are washed out
	let field = if focused {
		Style::new()
			.fg(Color::White)
			.bg(Color::Indexed(239))
			.add_modifier(Modifier::UNDERLINED)
	} else {
		Style::new().bg(Color::Indexed(236))
	};
	let cursor = Style::new().fg(Color::Black).bg(Color::White);
	if focused && text.is_selected() {
		// all of it selected: typing replaces it
		let shown = truncate(text.text(), width.saturating_sub(1));
		let used = shown.chars().count() + 1;
		return vec![
			Span::styled(shown, Style::new().fg(Color::Black).bg(Color::Indexed(117))),
			Span::styled(" ", cursor),
			Span::styled(" ".repeat(width.saturating_sub(used)), field),
		];
	}
	let (before, at, after) = text.visible(width.saturating_sub(1));
	let used = before.chars().count() + at.chars().count() + after.chars().count();
	let mut spans = vec![Span::styled(before, field)];
	spans.push(Span::styled(at, if focused { cursor } else { field }));
	spans.push(Span::styled(after, field));
	spans.push(Span::styled(" ".repeat(width.saturating_sub(used)), field));
	spans
}

fn draw_typed(
	frame: &mut Frame<'_>,
	area: Rect,
	typed: &mut Typed,
	hits: &mut Hits,
) {
	let width = 76.min(area.width.saturating_sub(2));
	let message = wrapped_height(std::slice::from_ref(&typed.message), width - 2);
	let rect = centered(area, width, message + 8);
	open(frame, rect, hits);
	let block = frame_block(lformat!("Delete"), Tone::Danger);
	let inner = block.inner(rect);
	frame.render_widget(block, rect);
	let [text, prompt, input, error, buttons] = Layout::vertical([
		Constraint::Length(message),
		Constraint::Length(2),
		Constraint::Length(1),
		Constraint::Length(1),
		Constraint::Length(2),
	])
	.areas(inner);
	frame.render_widget(
		Paragraph::new(typed.message.clone())
			.wrap(Wrap { trim: false })
			.style(Style::new().fg(Color::Red)),
		text,
	);
	frame.render_widget(
		Paragraph::new(Line::from(vec![
			Span::raw(lformat!("To confirm, type the name: ")),
			Span::styled(typed.expected.clone(), bold()),
		])),
		Rect {
			y: prompt.y + 1,
			height: 1,
			..prompt
		},
	);
	frame.render_widget(
		Line::from(input_spans(
			&typed.input,
			input.width as usize,
			typed.focus == 0,
		)),
		input,
	);
	hits.click(input, Target::TypedInput);
	if let Some(message) = &typed.error {
		frame.render_widget(
			Span::styled(message.clone(), Style::new().fg(Color::Red)),
			error,
		);
	}
	let labels = [lformat!("Delete"), lformat!("Cancel")];
	let row = Rect {
		y: buttons.y + 1,
		height: 1,
		..buttons
	};
	let focus = typed.focus.checked_sub(1);
	for (n, rect) in button_row(frame, row, &labels, focus, Some(0))
		.into_iter()
		.enumerate()
	{
		hits.click(rect, Target::Button(n + 1));
	}
}

fn draw_form(
	frame: &mut Frame<'_>,
	area: Rect,
	form: &mut Form,
	hits: &mut Hits,
) {
	let width = area.width.saturating_sub(4).min(84);
	let inner_width = width.saturating_sub(2);
	let label_width = form
		.fields
		.iter()
		.map(|f| f.label.chars().count())
		.max()
		.unwrap_or(0)
		.min(26) as u16
		+ 3;
	let input_width = inner_width.saturating_sub(label_width + 2);

	// lines of the scrolling part, with the field each belongs to
	let mut lines: Vec<(Line<'static>, Option<usize>)> = Vec::new();
	if let Some(note) = &form.note {
		for line in wrap(vec![Line::from(note.clone())], inner_width as usize) {
			lines.push((line, None));
		}
		lines.push((Line::default(), None));
	}
	let mut focus_line = 0;
	for (n, field) in form.fields.iter().enumerate() {
		let focused = n == form.focus;
		if focused {
			focus_line = lines.len();
		}
		// `›` marks the focused row without relying on colour
		let pointer = Span::styled(if focused { "›" } else { " " }, bold().fg(ACCENT));
		let marker = if field.required {
			Span::styled("* ", Style::new().fg(Color::Yellow))
		} else if field.missing {
			Span::styled("! ", Style::new().fg(Color::Yellow))
		} else {
			Span::raw("  ")
		};
		let label_style = if focused {
			Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
		} else {
			Style::new()
		};
		let mut spans = vec![
			pointer,
			marker,
			Span::styled(
				format!(
					"{:<w$}",
					truncate(&field.label, label_width as usize - 3),
					w = label_width as usize - 2
				),
				label_style,
			),
		];
		match &field.kind {
			FieldKind::Choice(options) => {
				let value = options
					.get(field.choice)
					.cloned()
					.unwrap_or_else(|| lformat!("(none)"));
				let style = if focused {
					Style::new().fg(Color::Black).bg(ACCENT)
				} else {
					Style::new().bg(Color::Indexed(236))
				};
				spans.push(Span::styled(format!("‹ {} ›", value), style));
				if options.len() > 1 {
					spans.push(Span::styled(
						format!("  {}/{}", field.choice + 1, options.len()),
						dim(),
					));
				}
			},
			FieldKind::Toggle => {
				let text = if field.on {
					format!("[x] {}", lformat!("yes"))
				} else {
					format!("[ ] {}", lformat!("no"))
				};
				let style = match (focused, field.locked.is_some()) {
					(_, true) => dim(),
					(true, false) => Style::new().fg(Color::Black).bg(ACCENT),
					(false, false) => Style::new(),
				};
				spans.push(Span::styled(text, style));
			},
			_ => spans.extend(input_spans(&field.input, input_width as usize, focused)),
		}
		lines.push((Line::from(spans), Some(n)));
		let indent = " ".repeat(label_width as usize + 1);
		if let Some(error) = &field.error {
			for line in wrap(
				vec![Line::from(Span::styled(
					format!("✗ {}", error),
					Style::new().fg(Color::Red),
				))],
				input_width as usize,
			) {
				let mut spans = vec![Span::raw(indent.clone())];
				spans.extend(line.spans);
				lines.push((Line::from(spans), Some(n)));
			}
		} else if focused && !field.hint.is_empty() {
			// long hints (the allowed titles) wrap under the input instead of being cut
			for line in wrap(
				vec![Line::from(Span::styled(field.hint.clone(), dim()))],
				input_width as usize,
			) {
				let mut spans = vec![Span::raw(indent.clone())];
				spans.extend(line.spans);
				lines.push((Line::from(spans), Some(n)));
			}
		}
	}
	if let Some(error) = &form.error {
		lines.push((Line::default(), None));
		lines.push((
			Line::from(Span::styled(error.clone(), Style::new().fg(Color::Red))),
			None,
		));
	}

	let wanted = lines.len() as u16 + 4;
	let rect = centered(area, width, wanted.min(area.height));
	open(frame, rect, hits);
	let block = panel(form.title.clone()).border_style(Style::new().fg(ACCENT));
	let inner = block.inner(rect);
	frame.render_widget(block, rect);
	let [body, buttons] = Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(inner);

	// keep the focused field in view
	let visible = body.height as usize;
	let mut scroll = form.scroll as usize;
	if form.focus < form.fields.len() {
		if focus_line < scroll {
			scroll = focus_line;
		} else if focus_line + 2 > scroll + visible {
			scroll = (focus_line + 2).saturating_sub(visible);
		}
	}
	scroll = scroll.min(lines.len().saturating_sub(visible));
	form.scroll = scroll as u16;
	for (row, (line, field)) in lines.iter().skip(scroll).take(visible).enumerate() {
		let rect = Rect::new(body.x, body.y + row as u16, body.width, 1);
		frame.render_widget(line.clone(), rect);
		if let Some(field) = field {
			hits.click(rect, Target::FormField(*field));
		}
	}
	hits.wheel(body, Wheel::Form);
	scrollbar(
		frame,
		Rect {
			y: body.y.saturating_sub(1),
			height: body.height + 2,
			..rect
		},
		lines.len(),
		visible,
		scroll,
	);

	let labels = [form.submit.clone(), lformat!("Cancel")];
	let focus = form.focus.checked_sub(form.fields.len());
	let row = Rect {
		y: buttons.y + 1,
		height: 1,
		..buttons
	};
	for (n, rect) in button_row(frame, row, &labels, focus, None)
		.into_iter()
		.enumerate()
	{
		hits.click(rect, Target::FormButton(n));
	}
}

fn draw_pick(
	frame: &mut Frame<'_>,
	area: Rect,
	pick: &mut Pick,
	hits: &mut Hits,
) {
	let longest = pick
		.items
		.iter()
		.map(|i| i.label.chars().count() + i.disabled.as_ref().map_or(0, |r| r.chars().count() + 3))
		.max()
		.unwrap_or(0);
	let width = (longest as u16 + 6).clamp(44, 90);
	let rect = centered(area, width, pick.items.len() as u16 + 4);
	open(frame, rect, hits);
	let block = panel(pick.title.clone())
		.border_style(Style::new().fg(ACCENT))
		.title_bottom(Line::from(lformat!(" Enter choose · Esc cancel ")).right_aligned());
	let inner = block.inner(rect);
	frame.render_widget(block, rect);
	let items: Vec<ListItem<'_>> = pick
		.items
		.iter()
		.map(|item| match &item.disabled {
			Some(reason) => ListItem::new(Line::from(vec![
				Span::styled(item.label.clone(), dim()),
				Span::styled(format!(" – {}", reason), dim()),
			])),
			None => ListItem::new(item.label.clone()),
		})
		.collect();
	let count = items.len();
	let list = List::new(items)
		.highlight_style(Style::new().fg(Color::Black).bg(ACCENT))
		.highlight_symbol("▶ ");
	frame.render_stateful_widget(list, inner, &mut pick.state);
	let offset = pick.state.offset();
	for n in 0..count.saturating_sub(offset).min(inner.height as usize) {
		hits.click(
			Rect::new(inner.x, inner.y + n as u16, inner.width, 1),
			Target::PickRow(offset + n),
		);
	}
	hits.wheel(inner, Wheel::Pick);
}

fn draw_output(
	frame: &mut Frame<'_>,
	area: Rect,
	title: &str,
	text: &str,
	scroll: &mut u16,
	hits: &mut Hits,
) {
	let rect = centered(area, area.width.saturating_sub(4), area.height);
	open(frame, rect, hits);
	let width = rect.width.saturating_sub(3) as usize;
	let lines = wrap(
		text.lines().map(|l| Line::from(l.to_owned())).collect(),
		width,
	);
	let visible = rect.height.saturating_sub(2) as usize;
	let total = lines.len();
	*scroll = (*scroll).min(total.saturating_sub(visible) as u16);
	let position = if total > visible {
		lformat!(
			" lines {}–{} of {} · ↑↓ PgUp PgDn · Esc closes ",
			*scroll as usize + 1,
			(*scroll as usize + visible).min(total),
			total
		)
	} else {
		lformat!(" Esc closes ")
	};
	frame.render_widget(
		Paragraph::new(lines)
			.block(
				panel(title.to_owned())
					.border_style(Style::new().fg(ACCENT))
					.title_bottom(Line::from(position).right_aligned()),
			)
			.scroll((*scroll, 0)),
		rect,
	);
	hits.wheel(rect, Wheel::Output);
	scrollbar(frame, rect, total, visible, *scroll as usize);
}
