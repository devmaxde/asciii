//! The dashboard: figures of the year, to-dos, the next 30 days.

use asciii::project::spec::HasEmployees;
use asciii::stats::{self, plus, TaskKind};
use bill::Currency;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, Block, BorderType, Cell, List, ListItem, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use super::super::actions::Action;
use super::super::app::App;
use super::super::mouse::{Hits, Target, Wheel};
use super::{dim, dmy, money, panel, urgency_style, ACCENT};

/// Below this the dashboard uses the compact layout.
const FULL_WIDTH: u16 = 100;
const FULL_HEIGHT: u16 = 30;

const RECEIVED: Color = Color::Green;
const WAITING: Color = Color::Yellow;
const NOT_INVOICED: Color = Color::Blue;

/// One of the three parts of the expected total; every euro is in exactly one of them.
struct Part {
    label: String,
    value: Currency,
    color: Color,
    /// counts, with the part that needs attention highlighted
    detail: Vec<Span<'static>>,
}

fn parts(app: &App) -> [Part; 3] {
    let s = &app.stats;
    let mut waiting = vec![Span::styled(if s.open_bills.count == 1 { lformat!("1 invoice") } else { lformat!("{} invoices", s.open_bills.count) }, dim())];
    if s.overdue_bills > 0 {
        waiting.push(Span::styled(", ", dim()));
        waiting.push(Span::styled(lformat!("{} overdue", s.overdue_bills), Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)));
    }
    let mut not_invoiced = vec![Span::styled(if s.open.count == 1 { lformat!("1 project") } else { lformat!("{} projects", s.open.count) }, dim())];
    if s.to_invoice.count > 0 {
        // a to-do count, not another sum: events that are over and still have no invoice
        not_invoiced.push(Span::styled(", ", dim()));
        not_invoiced.push(Span::styled(lformat!("{} already over", s.to_invoice.count), Style::new().fg(Color::Yellow)));
    }
    [
        Part {
            label: lformat!("Received"),
            value: s.received.incl_vat,
            color: RECEIVED,
            detail: vec![Span::styled(lformat!("{} paid", s.received.count), dim())],
        },
        Part {
            label: lformat!("Waiting for payment"),
            value: s.open_bills.incl_vat,
            color: WAITING,
            detail: waiting,
        },
        Part {
            label: lformat!("Not yet invoiced"),
            value: s.open.incl_vat,
            color: NOT_INVOICED,
            detail: not_invoiced,
        },
    ]
}

fn width_of(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// The expected total of the year and its three parts.
fn draw_money(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let parts = parts(app);
    // the total is the sum of the parts, so they visibly add up
    let total = parts.iter().fold(Currency::default(), |sum, p| plus(sum, p.value));
    let excl = plus(app.stats.invoiced.excl_vat, app.stats.open.excl_vat);
    let mut title = vec![
        Span::raw(" "),
        Span::styled(lformat!("Expected {}:", app.year), Style::new().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(money(total), Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
    ];
    let secondary = Span::styled(format!("({}) ", lformat!("{} excl. VAT", money(excl))), dim());
    if width_of(&title) + secondary.content.chars().count() + 4 <= area.width as usize {
        title.push(secondary);
    }
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(dim()).title(Line::from(title));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut y = inner.y;
    let label_width = parts.iter().map(|p| p.label.chars().count()).max().unwrap_or(0);
    let value_width = parts.iter().map(|p| money(p.value).chars().count()).max().unwrap_or(0);
    for part in &parts {
        if y >= inner.bottom() {
            break;
        }
        let rect = Rect::new(inner.x, y, inner.width, 1);
        let mut spans = vec![
            Span::styled("■ ", Style::new().fg(part.color)),
            Span::styled(format!("{:<w$}  ", part.label, w = label_width), Style::new().fg(part.color)),
            Span::styled(format!("{:>w$}", money(part.value), w = value_width), Style::new().add_modifier(Modifier::BOLD)),
        ];
        // the counts only when they fit completely
        if width_of(&spans) + 2 + width_of(&part.detail) <= inner.width as usize {
            spans.push(Span::raw("  "));
            spans.extend(part.detail.iter().cloned());
        }
        frame.render_widget(Line::from(spans), rect);
        hits.click(rect, Target::Action(Action::ScopeYear));
        y += 1;
    }
}

/// Wages are costs, kept apart from the revenue.
fn wages_lines(app: &App) -> Vec<Line<'static>> {
    let s = &app.stats;
    let (open, paid) = (lformat!("not paid yet"), lformat!("paid"));
    let w = open.chars().count().max(paid.chars().count()) + 1;
    vec![
        Line::from(vec![
            Span::styled(format!("{:<w$}", open), dim()),
            Span::styled(money(s.wages_open), Style::new().fg(if s.wages_open.value > 0 { Color::Yellow } else { Color::Green }).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![Span::styled(format!("{:<w$}", paid), dim()), Span::raw(money(s.wages_paid))]),
        Line::from(Span::styled(lformat!("{:.1} h service", s.service_hours), dim())),
    ]
}

fn projects_in_year(app: &App) -> usize {
    app.data
        .as_ref()
        .map(|d| d.entries.iter().filter(|e| stats::in_year(&e.project, app.year)).count())
        .unwrap_or(0)
}

/// "No projects in 2027": what to do instead of empty tiles.
fn draw_empty_year(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let block = panel(app.year.to_string());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = vec![
        Line::from(Span::styled(lformat!("No projects in {}.", app.year), Style::new().add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(
            lformat!("n creates one, ← and → change the year, 2 shows the list of all projects."),
            dim(),
        )),
    ];
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).centered(), inner);
    hits.click(inner, Target::Action(Action::NewProject));
}

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let empty_year = projects_in_year(app) == 0;
    if area.width < FULL_WIDTH || area.height < FULL_HEIGHT {
        return draw_compact(frame, app, area, hits, empty_year);
    }
    let [figures, rest] = Layout::vertical([Constraint::Length(5), Constraint::Fill(1)]).areas(area);
    if empty_year {
        draw_empty_year(frame, app, figures, hits);
    } else {
        let [money_area, wages_area] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(30)]).areas(figures);
        draw_money(frame, app, money_area, hits);
        frame.render_widget(Paragraph::new(wages_lines(app)).block(panel(lformat!("Wages (costs)"))), wages_area);
    }

    let [left, right] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).areas(rest);
    let [chart_area, upcoming_area] = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(left);
    let [tasks_area, wages_area] = Layout::vertical([Constraint::Fill(2), Constraint::Fill(1)]).areas(right);

    draw_chart(frame, app, chart_area);
    draw_upcoming(frame, app, upcoming_area, hits);
    draw_tasks(frame, app, tasks_area, hits);
    draw_open_wages(frame, app, wages_area);
}

/// Small terminals: the money block with one line for wages, the to-dos and the next 30 days;
/// no chart, no wages panel.
fn draw_compact(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits, empty_year: bool) {
    let [figures, wages, rest] =
        Layout::vertical([Constraint::Length(5), Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    if empty_year {
        draw_empty_year(frame, app, figures.union(wages), hits);
    } else {
        draw_money(frame, app, figures, hits);
        let s = &app.stats;
        let line = Line::from(vec![
            Span::styled(format!(" {}: ", lformat!("Wages (costs)")), dim()),
            Span::styled(lformat!("{} open", money(s.wages_open)), Style::new().fg(if s.wages_open.value > 0 { Color::Yellow } else { Color::Green })),
            Span::styled(" · ", dim()),
            Span::styled(lformat!("{} paid", money(s.wages_paid)), dim()),
        ]);
        frame.render_widget(line, wages);
    }
    let [tasks_area, upcoming_area] = if rest.width >= 90 {
        Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).areas(rest)
    } else {
        Layout::vertical([Constraint::Fill(3), Constraint::Fill(2)]).areas(rest)
    };
    draw_tasks(frame, app, tasks_area, hits);
    draw_upcoming(frame, app, upcoming_area, hits);
}

fn draw_chart(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let s = &app.stats;
    let months = [
        lformat!("Jan"),
        lformat!("Feb"),
        lformat!("Mar"),
        lformat!("Apr"),
        lformat!("May"),
        lformat!("Jun"),
        lformat!("Jul"),
        lformat!("Aug"),
        lformat!("Sep"),
        lformat!("Oct"),
        lformat!("Nov"),
        lformat!("Dec"),
    ];
    let inner_width = area.width.saturating_sub(2);
    let bar_width = (inner_width / 12).saturating_sub(1).max(1);
    let bars: Vec<Bar<'_>> = months
        .iter()
        .zip(s.monthly_invoiced.iter())
        .map(|(month, amount)| {
            let euros = (amount.value.max(0) / 100) as u64;
            let label = if euros >= 1000 { format!("{:.1}k", euros as f64 / 1000.0) } else { euros.to_string() };
            Bar::with_label(month.clone(), euros)
                .text_value(if euros == 0 { String::new() } else { label })
                .style(Style::new().fg(Color::Green))
        })
        .collect();
    let title = lformat!("Invoiced per month {} (by event date, incl. VAT, €)", s.year);
    let chart = BarChart::new(bars)
        .block(panel(title))
        .bar_width(bar_width)
        .bar_gap(1)
        .value_style(Style::new().fg(Color::Black).bg(Color::Green))
        .label_style(dim());
    frame.render_widget(chart, area);
}

fn draw_upcoming(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let narrow = area.width < 60;
    let rows: Vec<Row<'_>> = app
        .upcoming
        .iter()
        .filter_map(|&i| app.entry(i))
        .map(|e| {
            let staff = e.project.hours().employees().map(|v| v.len()).unwrap_or(0);
            let mut cells = vec![Cell::from(dmy(e.summary.date)), Cell::from(e.summary.name.clone())];
            if !narrow {
                cells.push(Cell::from(e.summary.manager.clone()));
                cells.push(Cell::from(if staff > 0 { lformat!("{} staff", staff) } else { String::new() }));
            }
            Row::new(cells).style(urgency_style(e.summary.urgency))
        })
        .collect();
    let count = rows.len();
    let widths: Vec<Constraint> = if narrow {
        vec![Constraint::Length(10), Constraint::Fill(1)]
    } else {
        vec![Constraint::Length(10), Constraint::Fill(2), Constraint::Fill(1), Constraint::Length(lformat!("{} staff", 99).chars().count() as u16)]
    };
    let block = panel(lformat!("Next 30 days"));
    let inner = block.inner(area);
    frame.render_widget(Table::new(rows, widths).block(block), area);
    if count == 0 {
        frame.render_widget(Paragraph::new(lformat!("No events in the next 30 days.")).style(dim()), inner);
        return;
    }
    for n in 0..count.min(inner.height as usize) {
        hits.click(Rect::new(inner.x, inner.y + n as u16, inner.width, 1), Target::UpcomingRow(n));
    }
}

fn task_text(kind: &TaskKind) -> (String, Color) {
    match kind {
        TaskKind::IssueInvoice => (lformat!("write invoice"), Color::Yellow),
        TaskKind::FollowUp { days } => (lformat!("payment {} days open", days), Color::Red),
        TaskKind::PayEmployees => (lformat!("pay employees"), Color::Magenta),
        TaskKind::Archive => (lformat!("archive"), Color::Cyan),
        TaskKind::DuplicateInvoiceNumber => (lformat!("⚠ duplicate invoice no."), Color::Red),
    }
}

fn draw_tasks(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let labelled: Vec<_> = app
        .tasks
        .iter()
        .map(|task| {
            let (text, color) = task_text(&task.kind);
            (task, text, color)
        })
        .collect();
    // widest label decides the column, so translations of any length line up
    let width = labelled.iter().map(|(_, text, _)| text.chars().count()).max().unwrap_or(0) + 1;
    let items: Vec<ListItem<'_>> = labelled
        .into_iter()
        .filter_map(|(task, text, color)| {
            let e = app.entry(task.index)?;
            let marker = if task.urgent { "! " } else { "· " };
            Some(ListItem::new(Line::from(vec![
                Span::styled(marker, Style::new().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<width$}", text, width = width), Style::new().fg(color)),
                Span::raw(format!("{:<5} ", e.summary.invoice_number)),
                Span::raw(e.summary.name.clone()),
            ])))
        })
        .collect();
    let title = lformat!("To-do ({}) · Enter opens", app.tasks.len());
    let block = panel(title);
    let inner = block.inner(area);
    if items.is_empty() {
        let text = vec![
            Line::from(Span::styled(lformat!("Nothing to do."), Style::new().fg(Color::Green))),
            Line::from(Span::styled(lformat!("Invoices, payments, wages and archiving show up here when they are due."), dim())),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(block), area);
        return;
    }
    let count = items.len();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, area, &mut app.task_state);
    let offset = app.task_state.offset();
    for n in 0..(count - offset.min(count)).min(inner.height as usize) {
        hits.click(Rect::new(inner.x, inner.y + n as u16, inner.width, 1), Target::TaskRow(offset + n));
    }
    hits.wheel(inner, Wheel::Tasks);
    super::scrollbar(frame, area, count, inner.height as usize, offset);
}

fn draw_open_wages(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows: Vec<Row<'_>> = app
        .stats
        .open_wages_by_employee
        .iter()
        .map(|(name, amount)| Row::new(vec![Cell::from(name.clone()), Cell::from(Line::from(money(*amount)).right_aligned())]))
        .collect();
    let title = lformat!("Open wages per employee");
    if rows.is_empty() {
        frame.render_widget(Paragraph::new(lformat!("All wages paid.")).style(dim()).block(panel(title)), area);
        return;
    }
    frame.render_widget(Table::new(rows, [Constraint::Fill(1), Constraint::Length(12)]).block(panel(title)), area);
}
