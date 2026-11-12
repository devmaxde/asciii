//! The Template tab: which template, its catalog and the starting offer of new projects.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use super::super::actions::{Action, Avail};
use super::super::app::App;
use super::super::mouse::{Hits, Target, Wheel};
use super::super::offer;
use super::super::template_editor::Row as TemplateRow;
use super::{bold, dim, key_style, panel, scrollbar, truncate, ACCENT};

/// What is special about a line of the starting offer: numbers left over from a project.
fn usage_note(line: &offer::Line) -> Option<String> {
    match (line.sold, line.returned) {
        (Some(sold), _) => Some(lformat!("starts with usage {}", offer::number(sold))),
        (None, Some(returned)) => Some(lformat!("starts with {} returned", offer::number(returned))),
        (None, None) => None,
    }
}

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    if app.template.is_none() {
        app.open_template();
    }
    let create = Action::CreateStorage.availability(app) == Avail::Enabled;
    let create_title = Action::CreateStorage.title(app);
    let Some(editor) = app.template.as_mut() else { return };

    let dirty = editor.dirty();
    let mut title = if editor.names.is_empty() { lformat!("Template") } else { lformat!("Template · {}", editor.name()) };
    if dirty {
        title.push_str(&format!(" · {}", lformat!("not saved (s saves)")));
    }
    let block = panel(title).border_style(Style::new().fg(if dirty { Color::Yellow } else { Color::DarkGray }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;

    // nothing to edit: say why and what helps
    if editor.names.is_empty() || editor.problem.is_some() {
        let mut lines = Vec::new();
        match &editor.problem {
            Some(problem) => {
                lines.push(Line::from(lformat!("The template cannot be shown.")));
                lines.push(Line::from(Span::styled(problem.clone(), Style::new().fg(Color::Red))));
            },
            None => {
                let folder = asciii::CONFIG.get_str_or("dirs/templates").unwrap_or("templates").to_owned();
                lines.push(Line::from(lformat!("There is no template in the folder \"{}\" yet.", folder)));
                lines.push(Line::from(Span::styled(lformat!("New projects are made from a template: its catalog and starting offer."), dim())));
            },
        }
        lines.push(Line::default());
        if create {
            lines.push(Line::from(vec![Span::styled(String::from("c  "), key_style()), Span::raw(create_title.clone())]));
        } else if editor.file.is_some() {
            lines.push(Line::from(vec![Span::styled(String::from("Shift+E  "), key_style()), Span::raw(lformat!("Edit the template file"))]));
        }
        let height = lines.len() as u16;
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        if create {
            hits.click(Rect::new(inner.x, inner.y + height - 1, inner.width, 1), Target::Action(Action::CreateStorage));
        }
        return;
    }

    let searching = editor.searching || !editor.search.is_empty();
    let scope = lformat!("New projects start with this. Existing projects keep their own catalog.");
    let scope_height = if scope.chars().count() > width { 2 } else { 1 };
    let [chooser_area, scope_area, search_area, table_area, sums_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(scope_height),
        Constraint::Length(u16::from(searching)),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .areas(inner);

    // which template: ‹ name ›, the configured default is marked
    let name = editor.name();
    let several = editor.names.len() > 1;
    let mut x = chooser_area.x;
    let mut put = |frame: &mut Frame<'_>, span: Span<'static>, target: Option<Target>| {
        let w = (span.width() as u16).min(chooser_area.right().saturating_sub(x));
        let rect = Rect::new(x, chooser_area.y, w, 1);
        frame.render_widget(span, rect);
        if let Some(target) = target {
            hits.click(rect, target);
        }
        x += w;
    };
    put(frame, Span::styled(format!("{} ", lformat!("Template:")), dim()), None);
    if several {
        put(frame, Span::styled("‹ ", bold().fg(ACCENT)), Some(Target::Action(Action::TemplatePrev)));
    }
    put(frame, Span::styled(name.clone(), bold()), None);
    if several {
        put(frame, Span::styled(" ›", bold().fg(ACCENT)), Some(Target::Action(Action::TemplateNext)));
    }
    if name == editor.default {
        put(frame, Span::styled(format!("  ★ {}", lformat!("default")), Style::new().fg(Color::Green)), None);
    }
    if several {
        let hint = lformat!("{} of {}, ←/→ chooses", editor.chosen + 1, editor.names.len());
        put(frame, Span::styled(format!("  {}", hint), dim()), None);
    }

    frame.render_widget(
        Paragraph::new(Span::styled(scope, dim())).wrap(Wrap { trim: true }),
        scope_area,
    );
    if searching {
        let cursor = if editor.searching { "▏" } else { "" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("/ ", key_style()),
                Span::raw(editor.search.text().to_owned()),
                Span::styled(cursor, Style::new().fg(ACCENT)),
            ])),
            search_area,
        );
    }

    // the table: catalog products with their starting quantity
    let rows = editor.rows();
    let start_label = lformat!("Starting offer");
    let start_width = start_label.chars().count().max(6) as u16;
    let notes = width >= 100;
    let fixed = 8 + 9 + start_width as usize + 3 + if notes { 26 } else { 0 };
    let name_width = width.saturating_sub(fixed).max(8);
    let right = |text: String| Cell::from(Line::from(text).right_aligned());
    let table_rows: Vec<Row<'_>> = rows
        .iter()
        .map(|&row| {
            let (name, unit, price) = editor.product(row);
            let line = editor.line_of(row).map(|i| &editor.offer.lines[i]);
            let mut note = line.and_then(usage_note);
            if matches!(row, TemplateRow::Loose(_)) {
                note = Some(note.map_or_else(|| lformat!("not in the catalog"), |n| format!("{}, {}", lformat!("not in the catalog"), n)));
            }
            let quantity = line.map(|l| offer::number(l.amount)).unwrap_or_default();
            let name_cell = match (&note, notes) {
                (Some(note), false) => {
                    let room = name_width.saturating_sub(name.chars().count() + 3);
                    if room >= 6 {
                        Cell::from(Line::from(vec![Span::raw(name), Span::styled(format!(" · {}", truncate(note, room)), Style::new().fg(Color::Yellow))]))
                    } else {
                        Cell::from(truncate(&name, name_width))
                    }
                },
                _ => Cell::from(truncate(&name, name_width)),
            };
            let mut cells = vec![
                name_cell,
                Cell::from(unit.unwrap_or_default()),
                right(offer::money(price)),
                Cell::from(Line::from(Span::styled(quantity, bold())).right_aligned()),
            ];
            if notes {
                cells.push(Cell::from(Span::styled(note.unwrap_or_default(), Style::new().fg(Color::Yellow))));
            }
            Row::new(cells)
        })
        .collect();

    let count = table_rows.len();
    if table_rows.is_empty() {
        let text = if searching {
            vec![Line::from(lformat!("No product matches the search.")), Line::from(Span::styled(lformat!("Esc clears the search."), dim()))]
        } else if !editor.has_catalog {
            let next = if editor.has_products {
                lformat!("n adds the first product; the catalog is started for it.")
            } else {
                lformat!("Add \"cataloge:\" with Edit the template file (Shift+E), then the products can be edited here.")
            };
            vec![Line::from(lformat!("This template has no catalog yet.")), Line::from(Span::styled(next, dim()))]
        } else {
            vec![
                Line::from(lformat!("The catalog of this template is empty.")),
                Line::from(Span::styled(lformat!("n adds a product."), dim())),
            ]
        };
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), table_area);
    } else {
        let mut header = vec![Cell::from(lformat!("Product")), Cell::from(lformat!("Unit")), right(lformat!("Price")), right(start_label)];
        let mut widths = vec![Constraint::Fill(1), Constraint::Length(8), Constraint::Length(9), Constraint::Length(start_width)];
        if notes {
            header.push(Cell::from(""));
            widths.push(Constraint::Length(24));
        }
        let table = Table::new(table_rows, widths)
            .header(Row::new(header).style(bold()))
            .column_spacing(1)
            .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT));
        frame.render_stateful_widget(table, table_area, &mut editor.table);
        let offset = editor.table.offset();
        let visible = table_area.height.saturating_sub(1) as usize;
        for n in 0..count.saturating_sub(offset).min(visible) {
            hits.click(Rect::new(table_area.x, table_area.y + 1 + n as u16, table_area.width, 1), Target::TemplateRow(offset + n));
        }
        hits.wheel(table_area, Wheel::Template);
        scrollbar(frame, area, count, visible, offset);
    }

    // what new projects get
    let offered = editor.offer.lines.len();
    let summary = if editor.has_products {
        lformat!(
            "{} products in the catalog · {} in the starting offer, {} before tax",
            editor.offer.catalog.len(),
            offered,
            offer::money(editor.offer.net())
        )
    } else {
        lformat!("{} products in the catalog · no starting offer (the template has no products section)", editor.offer.catalog.len())
    };
    frame.render_widget(
        Paragraph::new(vec![Line::default(), Line::from(Span::styled(truncate(&summary, width), dim()))]),
        sums_area,
    );
}
