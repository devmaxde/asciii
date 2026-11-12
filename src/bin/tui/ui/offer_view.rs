//! The offer and usage editor.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use super::super::app::App;
use super::super::mouse::{Hits, Target, Wheel};
use super::super::offer;
use super::super::offer_editor::EditorMode;
use super::{bold, dim, panel, scrollbar, ACCENT};

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let Some(editor) = app.offer.as_mut() else { return };
    let usage = editor.mode == EditorMode::Usage;
    let offer = &editor.offer;
    let template = editor.template.as_ref().map(|(_, catalog)| catalog);
    // the note column needs room; on narrow terminals it goes
    let notes = area.width >= 100;

    let mut title = if usage { lformat!("Actual usage · {}", editor.name) } else { lformat!("Offer · {}", editor.name) };
    if editor.dirty {
        title.push_str(&format!(" · {}", lformat!("not saved (s saves)")));
    }
    let block = panel(title).border_style(Style::new().fg(if editor.dirty { Color::Yellow } else { Color::DarkGray }));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [table_area, sums_area] = Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(inner);

    let right = |text: String| Cell::from(Line::from(text).right_aligned());
    let rows: Vec<Row<'_>> = offer
        .lines
        .iter()
        .enumerate()
        .map(|(n, line)| {
            let (name, unit, price) = offer.resolved(line);
            let mut cells = vec![right(format!("{}", n + 1)), Cell::from(name), Cell::from(unit.unwrap_or_default()), right(offer::money(price))];
            if usage {
                let used = line.used();
                let difference = used - line.amount;
                let (note, style) = if !line.counted() {
                    (lformat!("not counted yet"), dim())
                } else if difference > 0.0 {
                    (lformat!("{} more than offered", offer::number(difference)), Style::new().fg(Color::Yellow))
                } else if difference < 0.0 {
                    (lformat!("{} back", offer::number(-difference)), Style::new().fg(Color::Green))
                } else {
                    (lformat!("as offered"), Style::new())
                };
                cells.extend([
                    right(offer::number(line.amount)),
                    Cell::from(Line::from(Span::styled(offer::number(used), if line.counted() { bold() } else { dim() })).right_aligned()),
                    right(offer::money(price * used)),
                ]);
                if notes {
                    cells.push(Cell::from(Span::styled(note, style)));
                }
            } else {
                // how the product compares with the template: special prices are easy to see
                let note = match (line.anchor.as_deref().and_then(|a| offer.item(a)), template) {
                    (Some(item), Some(template)) => match template.iter().find(|t| t.anchor == item.anchor) {
                        None => Span::styled(lformat!("not in template"), Style::new().fg(Color::Yellow)),
                        Some(t) if t.price != item.price => {
                            Span::styled(lformat!("template {}", offer::money(t.price)), Style::new().fg(Color::Yellow))
                        },
                        Some(_) => Span::raw(""),
                    },
                    (None, _) => Span::styled(lformat!("without catalog"), dim()),
                    _ => Span::raw(""),
                };
                let used = if line.counted() { lformat!("used {}", offer::number(line.used())) } else { String::new() };
                cells.extend([
                    right(offer::number(line.amount)),
                    Cell::from(Line::from(Span::styled(used, dim())).right_aligned()),
                    right(offer::money(price * line.amount)),
                ]);
                if notes {
                    cells.push(Cell::from(note));
                }
            }
            Row::new(cells)
        })
        .collect();

    let count = rows.len();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(lformat!("No products yet.")),
                Line::from(Span::styled(lformat!("a adds one from the catalog, n a new one; Ctrl+P lists everything."), dim())),
            ])
            .wrap(Wrap { trim: true }),
            table_area,
        );
    } else {
        let (quantity, second) = if usage { (lformat!("Offered"), lformat!("Used")) } else { (lformat!("Quantity"), String::new()) };
        let mut header = vec![
            right(String::from("#")),
            Cell::from(lformat!("Product")),
            Cell::from(lformat!("Unit")),
            right(lformat!("Price")),
            right(quantity),
            right(second),
            right(lformat!("Sum")),
        ];
        let mut widths = vec![
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(10),
        ];
        if notes {
            header.push(Cell::from(""));
            widths.push(Constraint::Length(24));
        }
        let table = Table::new(rows, widths)
            .header(Row::new(header).style(bold()))
            .column_spacing(if notes { 2 } else { 1 })
            .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT));
        frame.render_stateful_widget(table, table_area, &mut editor.table);
        let offset = editor.table.offset();
        let visible = table_area.height.saturating_sub(1) as usize;
        for n in 0..count.saturating_sub(offset).min(visible) {
            hits.click(Rect::new(table_area.x, table_area.y + 1 + n as u16, table_area.width, 1), Target::OfferRow(offset + n));
        }
        hits.wheel(table_area, Wheel::Offer);
        scrollbar(frame, area, count, visible, offset);
    }

    let net = if usage { offer.used_net() } else { offer.net() };
    let label = if usage { lformat!("Used, before tax") } else { lformat!("Sum before tax") };
    let note = if usage {
        match editor.uncounted() {
            0 => lformat!("Every product is counted. s saves, then the invoice is next."),
            n => lformat!("{} products not counted yet: saving counts them as used like offered.", n),
        }
    } else {
        lformat!("Products only; staff hours and own-tax products are added in the PDF.")
    };
    let sums = vec![
        Line::default(),
        Line::from(vec![
            Span::styled(format!("{:<18}", label), dim()),
            Span::styled(offer::money(net), bold()),
            Span::styled(format!("   {} {}% ", lformat!("plus VAT"), offer::number(editor.tax * 100.0)), dim()),
            Span::raw(offer::money(net * editor.tax)),
            Span::styled(format!("   {} ", lformat!("total")), dim()),
            Span::styled(offer::money(net * (1.0 + editor.tax)), bold()),
        ]),
        Line::from(Span::styled(note, dim())),
    ];
    frame.render_widget(Paragraph::new(sums), sums_area);
}
