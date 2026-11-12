//! The project list: a filter bar and the table.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use ratatui::Frame;

use super::super::actions::Action;
use super::super::app::{App, Scope, SortBy};
use super::super::data::{Location, Payment};
use super::super::lifecycle;
use super::super::mouse::{Hits, Target, Wheel};
use super::{bold, check, dim, dmy, key_style, panel, truncate, urgency_style, ACCENT};

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let shown = app.rows.len();
    let block = panel(lformat!("Projects · {} shown", shown));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [bar, table_area] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    draw_filter_bar(frame, app, bar, hits);
    if app.scope == Scope::Broken {
        draw_broken(frame, app, table_area, hits);
    } else {
        draw_table(frame, app, table_area, hits);
    }
    hits.wheel(table_area, Wheel::Projects);
    let visible = table_area.height.saturating_sub(1) as usize;
    super::scrollbar(frame, area, app.rows.len(), visible, app.table.offset());
}

/// `Show: ‹2026› ‹All years› ‹Broken (4)›   Archived [x]   Sort: date ↓   Search: …`, every part clickable.
fn draw_filter_bar(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    // narrow terminals and longer (German) words: smaller gaps first, then without the "Show:" label
    let options = [(3, true), (2, true), (1, true), (1, false)];
    let parts = options
        .iter()
        .map(|&(gap, label)| filter_parts(app, gap, label))
        .find(|parts| parts.iter().map(|(s, _)| s.width()).sum::<usize>() <= area.width as usize)
        .unwrap_or_else(|| filter_parts(app, 1, false));
    let mut x = area.x;
    for (span, target) in parts {
        if span.width() == 0 {
            continue;
        }
        let width = (span.width() as u16).min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        frame.render_widget(span, rect);
        if let Some(target) = target {
            hits.click(rect, target);
        }
        x += width;
    }
}

fn filter_parts(app: &App, gap: usize, show_label: bool) -> Vec<(Span<'static>, Option<Target>)> {
    let gap = " ".repeat(gap);
    let broken = app.data.as_ref().map_or(0, |d| d.broken.len());
    let chip = |text: String, active: bool| {
        let style = if active { Style::new().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::new() };
        Span::styled(format!("‹{}›", text), style)
    };
    let mut parts: Vec<(Span<'static>, Option<Target>)> = vec![
        (Span::styled(if show_label { format!("{} ", lformat!("Show:")) } else { String::new() }, dim()), None),
        (chip(app.year.to_string(), app.scope == Scope::Year), Some(Target::Scope(Scope::Year))),
        (Span::raw(" "), None),
        (chip(lformat!("All years"), app.scope == Scope::AllYears), Some(Target::Scope(Scope::AllYears))),
    ];
    if broken > 0 || app.scope == Scope::Broken {
        parts.push((Span::raw(" "), None));
        let mut span = chip(lformat!("Broken ({})", broken), app.scope == Scope::Broken);
        if app.scope != Scope::Broken {
            span = span.style(Style::new().fg(Color::Red));
        }
        parts.push((span, Some(Target::Scope(Scope::Broken))));
    }
    if app.scope != Scope::Broken {
        parts.push((Span::raw(gap.clone()), None));
        parts.push((
            Span::raw(format!("{} {}", lformat!("Archived"), if app.show_archived { "[x]" } else { "[ ]" })),
            Some(Target::Action(Action::ToggleArchived)),
        ));
        parts.push((Span::raw(gap.clone()), None));
        parts.push((
            Span::raw(format!("{} {} {}", lformat!("Sort:"), app.sort.label(), if app.sort_descending { "↓" } else { "↑" })),
            Some(Target::Action(Action::Sort)),
        ));
        parts.push((Span::raw(gap.clone()), None));
        if app.searching || !app.search.is_empty() {
            let (before, at, after) = app.search.visible(20);
            parts.push((Span::styled(format!("{} ", lformat!("Search:")), dim()), Some(Target::Action(Action::Search))));
            parts.push((Span::styled(before, Style::new().fg(Color::Yellow)), None));
            if app.searching {
                parts.push((Span::styled(at, Style::new().fg(Color::Black).bg(Color::Yellow)), None));
            }
            parts.push((Span::styled(after, Style::new().fg(Color::Yellow)), None));
            if !app.searching {
                parts.push((Span::styled(format!("  {}", lformat!("(Esc clears)")), dim()), None));
            }
        } else {
            parts.push((Span::styled(format!("{} ", lformat!("Search:")), dim()), Some(Target::Action(Action::Search))));
            parts.push((Span::styled("/", key_style()), Some(Target::Action(Action::Search))));
        }
    } else {
        parts.push((Span::styled(format!("   {}", lformat!("Enter shows why a folder is broken")), dim()), None));
    }
    parts
}

fn draw_broken(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let Some(data) = &app.data else { return };
    let rows: Vec<Row<'_>> = app
        .rows
        .iter()
        .filter_map(|&i| data.broken.get(i))
        .map(|b| {
            let location = match b.location {
                Location::Working => lformat!("working"),
                Location::Archive(year) => year.to_string(),
            };
            Row::new(vec![
                Cell::from(b.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                Cell::from(location),
                Cell::from(Span::styled(b.error.replace('\n', " "), Style::new().fg(Color::Red))),
            ])
        })
        .collect();
    let count = rows.len();
    let table = Table::new(rows, [Constraint::Fill(1), Constraint::Length((lformat!("working").chars().count() as u16).max(lformat!("Where").chars().count() as u16).max(4)), Constraint::Fill(2)])
        .header(Row::new(vec![lformat!("Folder"), lformat!("Where"), lformat!("Problem (Enter: all of it)")]).style(bold()))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(table, area, &mut app.table);
    row_hits(app, area, count, hits);
    if count == 0 {
        frame.render_widget(Paragraph::new(lformat!("All project folders can be read.")).style(dim()).centered(), below_header(area));
    }
}

fn below_header(area: Rect) -> Rect {
    Rect {
        y: area.y + 2,
        height: area.height.saturating_sub(2),
        ..area
    }
}

fn row_hits(app: &App, area: Rect, count: usize, hits: &mut Hits) {
    let offset = app.table.offset();
    let visible = area.height.saturating_sub(1) as usize;
    for n in 0..count.saturating_sub(offset).min(visible) {
        hits.click(Rect::new(area.x, area.y + 1 + n as u16, area.width, 1), Target::ProjectRow(offset + n));
    }
}

/// Which columns fit: the name keeps at least this much room.
const NAME_MIN: u16 = 18;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Git,
    Index,
    Name,
    Manager,
    Number,
    Date,
    Next,
    Offer,
    Invoice,
    Paid,
    Archive,
    Amount,
}

/// Wide enough for every step of the "Next" column, in the language shown.
fn next_width() -> u16 {
    let steps = lifecycle::Step::ALL.iter().map(|s| s.todo().chars().count());
    steps.chain([lformat!("Next").chars().count(), lformat!("archived").chars().count()]).max().unwrap_or(14) as u16
}

/// The columns for `width`; dropped in this order on narrow terminals: manager, the four checks, number.
fn columns(width: u16) -> Vec<Column> {
    use Column::*;
    let checks: u16 = [lformat!("Offer"), lformat!("Invoice"), lformat!("Paid"), lformat!("Archive")]
        .iter()
        .map(|h| h.chars().count() as u16 + 1)
        .sum();
    let base = 1 + 3 + 10 + next_width() + 10 + 6 + NAME_MIN;
    let mut columns = vec![Git, Index, Name];
    if width >= base + 7 + checks + 13 {
        columns.push(Manager);
    }
    if width >= base + 7 {
        columns.push(Number);
    }
    columns.extend([Date, Next]);
    if width >= base + 7 + checks {
        columns.extend([Offer, Invoice, Paid, Archive]);
    }
    columns.push(Amount);
    columns
}

fn header(column: Column) -> (String, Constraint, Option<SortBy>) {
    use Column::*;
    let fit = |text: String| {
        let width = text.chars().count() as u16;
        (text, Constraint::Length(width.max(1)), None)
    };
    match column {
        Git => (String::new(), Constraint::Length(1), None),
        Index => (String::from("#"), Constraint::Length(3), None),
        Name => (lformat!("Name"), Constraint::Fill(3), None),
        Manager => (lformat!("Manager"), Constraint::Fill(1), None),
        Number => (lformat!("No."), Constraint::Length(6), Some(SortBy::Number)),
        Date => (lformat!("Date"), Constraint::Length(10), Some(SortBy::Date)),
        Next => (lformat!("Next"), Constraint::Length(next_width()), None),
        Offer => fit(lformat!("Offer")),
        Invoice => fit(lformat!("Invoice")),
        Paid => fit(lformat!("Paid")),
        Archive => fit(lformat!("Archive")),
        Amount => (lformat!("Amount"), Constraint::Length(10), None),
    }
}

fn draw_table(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let Some(data) = &app.data else { return };
    let columns = columns(area.width);
    let headers: Vec<(String, Constraint, Option<SortBy>)> = columns.iter().map(|c| header(*c)).collect();
    let widths: Vec<Constraint> = headers.iter().map(|(_, w, _)| *w).collect();
    // the same layout the table uses: for clickable headers and to shorten long names with …
    let cells = Layout::horizontal(widths.clone()).flex(Flex::Start).spacing(1).split(Rect { height: 1, ..area });
    let room = |column: Column| columns.iter().position(|c| *c == column).map_or(0, |i| cells[i].width as usize);
    let (name_room, manager_room) = (room(Column::Name), room(Column::Manager));
    let rows: Vec<Row<'_>> = app
        .rows
        .iter()
        .enumerate()
        .filter_map(|(n, &i)| data.entries.get(i).map(|e| (n, e)))
        .map(|(n, e)| {
            let s = &e.summary;
            let archived = matches!(e.location, Location::Archive(_));
            // archived projects are done, keep them in view but quiet
            let style = if archived { Style::new().fg(Color::DarkGray) } else { urgency_style(s.urgency) };
            let cells: Vec<Cell<'_>> = columns
                .iter()
                .map(|column| match column {
                    Column::Git => {
                        let (color, modifier) = git_style(&s.git);
                        Cell::from(Span::styled(s.git.to_string(), Style::new().fg(color).add_modifier(modifier)))
                    },
                    Column::Index => Cell::from(Line::from((n + 1).to_string()).right_aligned()),
                    Column::Name => Cell::from(Span::styled(
                        truncate(&if s.canceled { lformat!("CANCELED: {}", s.name) } else { s.name.clone() }, name_room),
                        style,
                    )),
                    Column::Manager => Cell::from(Span::styled(truncate(&s.manager, manager_room), style)),
                    // duplicate invoice numbers are illegal, make them impossible to miss
                    Column::Number if s.duplicate_of.is_empty() => Cell::from(Span::styled(s.invoice_number.clone(), style)),
                    Column::Number => Cell::from(Span::styled(
                        format!("⚠{}", s.invoice_number),
                        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                    )),
                    // listed in this year because of its invoice date, the event was in another year
                    Column::Date if app.scope == Scope::Year && !asciii::stats::in_year(&e.project, app.year) => {
                        Cell::from(Span::styled(dmy(s.date), style.add_modifier(Modifier::ITALIC | Modifier::DIM)))
                    },
                    Column::Date => Cell::from(Span::styled(dmy(s.date), style)),
                    // an archived project has no next step, even if it was archived before it was done
                    Column::Next if archived => Cell::from(Span::styled(lformat!("archived"), dim())),
                    Column::Next => Cell::from(match s.next {
                        // steps that need someone to act stand out, waiting ones do not
                        Some(step @ (lifecycle::Step::Usage | lifecycle::Step::Invoice)) => {
                            Span::styled(step.todo(), Style::new().fg(Color::Yellow))
                        },
                        Some(step) => Span::styled(step.todo(), style),
                        None => Span::styled(lformat!("done"), dim()),
                    }),
                    Column::Offer => Cell::from(check(s.offer_ready, s.offer_file)),
                    Column::Invoice => Cell::from(check(s.invoice_ready, s.invoice_file)),
                    Column::Paid => Cell::from(match s.payment {
                        Payment::Nothing => Span::styled("✗", Style::new().fg(Color::Red)),
                        Payment::Partial => Span::styled("€", Style::new().fg(Color::Yellow)),
                        Payment::Complete => Span::styled("€", Style::new().fg(Color::Green)),
                    }),
                    Column::Archive if archived => Cell::from(Span::styled("▪", Style::new().fg(Color::Blue))),
                    Column::Archive => Cell::from(check(s.archive_ready, false)),
                    Column::Amount => Cell::from(
                        Line::from(Span::styled(s.amount.clone(), if archived { style } else { Style::new() })).right_aligned(),
                    ),
                })
                .collect();
            Row::new(cells)
        })
        .collect();

    let header_row = Row::new(headers.iter().map(|(text, _, sort)| {
        let marker = match sort {
            Some(sort) if *sort == app.sort => if app.sort_descending { " ↓" } else { " ↑" },
            _ => "",
        };
        Cell::from(format!("{}{}", text, marker))
    }))
    .style(bold());

    let count = rows.len();
    let table = Table::new(rows, widths)
        .header(header_row)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(table, area, &mut app.table);

    for ((_, _, sort), rect) in headers.iter().zip(cells.iter()) {
        if let Some(sort) = sort {
            hits.click(*rect, Target::Header(*sort));
        }
    }
    row_hits(app, area, count, hits);

    if count == 0 {
        let message = if app.search.is_empty() {
            lformat!("No projects here. ‹All years› or ← → show other years, n creates a project.")
        } else {
            lformat!("Nothing matches \"{}\". Esc clears the search.", app.search.text())
        };
        frame.render_widget(Paragraph::new(truncate(&message, area.width as usize)).style(dim()).centered(), below_header(area));
    }
}

fn git_style(status: &asciii::storage::repo::GitStatus) -> (Color, Modifier) {
    use asciii::storage::repo::GitStatus::*;
    match status {
        Current => (Color::Blue, Modifier::empty()),
        Conflict | IndexDeleted => (Color::Red, Modifier::empty()),
        WorkingNew => (Color::Green, Modifier::empty()),
        WorkingModified => (Color::Yellow, Modifier::empty()),
        IndexNew => (Color::Green, Modifier::BOLD),
        IndexModified => (Color::Blue, Modifier::BOLD),
        _ => (Color::White, Modifier::empty()),
    }
}
