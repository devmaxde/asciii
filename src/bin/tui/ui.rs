//! Drawing. Every clickable thing registers its area in `app.hits` while it is drawn.

use bill::Currency;
use chrono::prelude::*;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use ratatui::Frame;

use asciii::CONFIG;

use super::actions::{self, Action, Ctx, HINT_GAP};
use super::app::{App, Screen, ToastKind};
use super::data::Urgency;
use super::logger;
use super::mouse::{Hits, Target};

mod dashboard;
mod detail;
mod first_run;
mod help;
mod log_view;
mod modals;
mod offer_view;
mod projects;
mod template_view;

pub use self::help::help_lines;

pub const ACCENT: Color = Color::Cyan;
/// smallest size the layout works with
pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 15;

pub fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// How keys are written in hints.
pub fn key_style() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}

/// Formats money with the configured currency symbol, also for sums that carry none.
pub fn money(c: Currency) -> String {
    let c = Currency {
        symbol: c.symbol.or_else(|| CONFIG.get_char("currency")),
        value: c.value,
    };
    c.postfix().to_string()
}

pub fn dmy(date: Option<Date<Utc>>) -> String {
    date.map(|d| d.format("%d.%m.%Y").to_string()).unwrap_or_default()
}

pub fn check(ok: bool, file: bool) -> Span<'static> {
    if ok {
        let style = Style::new().fg(Color::Green);
        Span::styled("✓", if file { style.add_modifier(Modifier::BOLD) } else { style })
    } else {
        Span::styled("✗", Style::new().fg(Color::Red))
    }
}

pub fn urgency_style(urgency: Urgency) -> Style {
    match urgency {
        Urgency::Normal => Style::new(),
        Urgency::Canceled => dim().add_modifier(Modifier::CROSSED_OUT),
        Urgency::Old => Style::new().fg(Color::Magenta),
        Urgency::Recent => Style::new().fg(Color::Cyan),
        Urgency::Today => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        Urgency::ThisWeek => Style::new().fg(Color::Red),
        Urgency::NextWeek => Style::new().fg(Color::Yellow),
        Urgency::Later => Style::new().fg(Color::Green),
        Urgency::Undated => Style::new().fg(Color::Red),
    }
}

pub fn panel(title: String) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(dim())
        .title(Span::styled(format!(" {} ", title), bold()))
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height).max(1);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// `text` cut to `max` characters, with `…` at the end.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else if max == 0 {
        String::new()
    } else {
        let mut cut: String = text.chars().take(max - 1).collect();
        cut.push('…');
        cut
    }
}

/// A vertical scrollbar on the right border of `area`, if `total` lines do not fit into `visible`.
pub fn scrollbar(frame: &mut Frame<'_>, area: Rect, total: usize, visible: usize, position: usize) {
    if total <= visible || area.height < 3 {
        return;
    }
    let mut state = ScrollbarState::new(total.saturating_sub(visible)).position(position).viewport_content_length(visible);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(dim())
            .thumb_style(Style::new().fg(ACCENT)),
        area.inner(Margin { vertical: 1, horizontal: 0 }),
        &mut state,
    );
}

/// Wraps styled lines at word boundaries to `width` columns; continuation lines keep the indentation.
pub fn wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    wrap_hanging(lines, width, 0)
}

/// Like `wrap`, but lines that start with blanks continue at column `hang` (for key/description tables).
pub fn wrap_hanging(lines: Vec<Line<'static>>, width: usize, hang: usize) -> Vec<Line<'static>> {
    let width = width.max(10);
    let mut out = Vec::new();
    for line in lines {
        if line.width() <= width {
            out.push(line);
            continue;
        }
        // the indentation of the first span is kept for the continuation lines
        let leading = line
            .spans
            .first()
            .map(|s| s.content.chars().take_while(|c| *c == ' ').count())
            .unwrap_or(0);
        let indent = if hang > 0 && leading > 0 { hang } else { leading }.min(width / 2);
        let style = line.style;
        let mut current: Vec<Span<'static>> = Vec::new();
        let mut used = 0;
        for span in line.spans {
            let mut word = String::new();
            let flush_word = |word: &mut String, current: &mut Vec<Span<'static>>, used: &mut usize, out: &mut Vec<Line<'static>>| {
                if word.is_empty() {
                    return;
                }
                let len = word.chars().count();
                if *used + len > width && *used > indent {
                    out.push(Line::from(std::mem::take(current)).style(style));
                    current.push(Span::raw(" ".repeat(indent)));
                    *used = indent;
                    let trimmed = word.trim_start().to_owned();
                    *word = trimmed;
                }
                // a word longer than the line is cut
                while *used + word.chars().count() > width {
                    let take = width.saturating_sub(*used).max(1);
                    let head: String = word.chars().take(take).collect();
                    let tail: String = word.chars().skip(take).collect();
                    current.push(Span::styled(head, span.style));
                    out.push(Line::from(std::mem::take(current)).style(style));
                    current.push(Span::raw(" ".repeat(indent)));
                    *used = indent;
                    *word = tail;
                }
                *used += word.chars().count();
                current.push(Span::styled(std::mem::take(word), span.style));
            };
            for c in span.content.chars() {
                if c == ' ' && !word.is_empty() && !word.ends_with(' ') {
                    flush_word(&mut word, &mut current, &mut used, &mut out);
                }
                word.push(c);
            }
            flush_word(&mut word, &mut current, &mut used, &mut out);
        }
        if !current.is_empty() {
            out.push(Line::from(current).style(style));
        }
    }
    out
}

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let mut hits = std::mem::take(&mut app.hits);
    hits.clear();
    let area = frame.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = vec![
            Line::from(lformat!("The terminal is too small.")),
            Line::from(lformat!("asciii needs at least {}×{}, this one is {}×{}.", MIN_WIDTH, MIN_HEIGHT, area.width, area.height)),
            Line::from(Span::styled(lformat!("Make the window larger, or press Ctrl+Q to quit."), dim())),
        ];
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).centered(), centered(area, area.width, 4));
        app.hits = hits;
        return;
    }

    let toast_height = u16::from(app.toast.is_some());
    let [top, body, toast, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(toast_height),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_top(frame, app, top, &mut hits);

    if app.screen == Screen::Log {
        log_view::draw(frame, app, body, &mut hits);
    } else if app.screen == Screen::Template {
        template_view::draw(frame, app, body, &mut hits);
    } else if app.data.is_none() {
        first_run::draw(frame, app, body, &mut hits);
    } else {
        match app.screen {
            Screen::Dashboard => dashboard::draw(frame, app, body, &mut hits),
            Screen::Projects => projects::draw(frame, app, body, &mut hits),
            Screen::Detail => detail::draw(frame, app, body, &mut hits),
            Screen::Offer => offer_view::draw(frame, app, body, &mut hits),
            Screen::Log | Screen::Template => {},
        }
    }

    draw_toast(frame, app, toast);
    draw_footer(frame, app, bottom, &mut hits);

    if app.modal.is_some() {
        modals::draw(frame, app, body, &mut hits);
    }
    app.hits = hits;
}

/// Tabs with the path to where the user is (`Projects › Sommerball › Offer`), year and git on the right.
fn draw_top(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    // right side first, it decides how much room the tabs have
    let ctx = app.ctx();
    let year_works = matches!(ctx, Ctx::Dashboard | Ctx::Projects | Ctx::Broken);
    let mut right: Vec<(Span<'static>, Option<Target>)> = Vec::new();
    if app.data.is_some() {
        if year_works {
            right.push((Span::styled("◀ ", bold().fg(ACCENT)), Some(Target::Action(Action::YearPrev))));
            right.push((Span::styled(app.year.to_string(), bold()), None));
            right.push((Span::styled(" ▶", bold().fg(ACCENT)), Some(Target::Action(Action::YearNext))));
        } else {
            right.push((Span::styled(app.year.to_string(), dim()), None));
        }
    }
    if let Some(data) = &app.data {
        right.push((Span::raw("  "), None));
        if data.git {
            let style = if data.git_changes > 0 { Style::new().fg(Color::Yellow) } else { dim() };
            right.push((Span::styled(lformat!("git: {} changed", data.git_changes), style), Some(Target::Action(Action::GitMenu))));
            // result of the startup `git fetch`
            if let Some(super::checks::Remote::Compared { behind, ahead }) = &app.remote {
                if *behind > 0 {
                    right.push((Span::styled(format!(" ↓{}", behind), Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)), None));
                }
                if *ahead > 0 {
                    right.push((Span::styled(format!(" ↑{}", ahead), Style::new().fg(Color::Yellow)), None));
                }
            }
        } else {
            right.push((Span::styled(lformat!("no git"), dim()), None));
        }
    }
    if app.is_loading() {
        right.push((Span::styled(format!("  {}", lformat!("loading …")), Style::new().fg(Color::Yellow)), None));
    }
    let right_width: u16 = right.iter().map(|(s, _)| s.width() as u16).sum();
    let mut x = area.right().saturating_sub(right_width);
    for (span, target) in &right {
        let width = span.width() as u16;
        let rect = Rect::new(x, area.y, width, 1);
        frame.render_widget(span.clone(), rect);
        if let Some(target) = target {
            hits.click(rect, *target);
        }
        x += width;
    }

    // tabs with the breadcrumb inside the Projects tab
    let room = area.width.saturating_sub(right_width + 2) as usize;
    let problems = logger::problem_count();
    let log_title = if problems > 0 { lformat!("Log ({})", problems) } else { lformat!("Log") };
    let template_title = match app.template.as_ref().filter(|t| t.dirty()) {
        Some(_) => format!("{} •", lformat!("Template")),
        None => lformat!("Template"),
    };
    let mut crumbs = vec![lformat!("Projects")];
    if matches!(app.screen, Screen::Detail | Screen::Offer) {
        if let Some(entry) = app.detail.and_then(|i| app.entry(i)) {
            crumbs.push(entry.summary.name.clone());
        }
        if app.screen == Screen::Offer {
            crumbs.push(match app.ctx() {
                Ctx::Usage => lformat!("Actual usage"),
                _ => lformat!("Offer"),
            });
        }
    }
    let tabs = [
        (Screen::Dashboard, lformat!("Dashboard")),
        (Screen::Projects, crumbs.join(" › ")),
        (Screen::Template, template_title),
        (Screen::Log, log_title),
    ];
    let fixed: usize = tabs.iter().map(|(_, t)| t.chars().count() + 3).sum::<usize>() - tabs[1].1.chars().count();
    let projects_room = room.saturating_sub(fixed).max(10);
    let selected = match app.screen {
        Screen::Dashboard => Screen::Dashboard,
        Screen::Projects | Screen::Detail | Screen::Offer => Screen::Projects,
        Screen::Log => Screen::Log,
        Screen::Template => Screen::Template,
    };
    let mut x = area.x;
    for (n, (screen, title)) in tabs.iter().enumerate() {
        if n > 0 {
            frame.render_widget(Span::styled("│", dim()), Rect::new(x, area.y, 1, 1));
            x += 1;
        }
        let title = if *screen == Screen::Projects { super::paths::middle(title, projects_room) } else { title.clone() };
        let text = format!(" {} ", title);
        let width = (text.chars().count() as u16).min(area.right().saturating_sub(x));
        let style = if *screen == selected { Style::new().fg(ACCENT).add_modifier(Modifier::BOLD) } else { dim() };
        let rect = Rect::new(x, area.y, width, 1);
        frame.render_widget(Span::styled(text, style), rect);
        hits.click(rect, Target::Tab(*screen));
        x += width;
    }
}

fn draw_toast(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(toast) = &app.toast else { return };
    if area.height == 0 {
        return;
    }
    let (mark, style) = match toast.kind {
        ToastKind::Success => ("✓ ", Style::new().fg(Color::Green)),
        ToastKind::Info => ("› ", Style::new().fg(Color::Yellow)),
        ToastKind::Error => ("✗ ", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
    };
    let mut spans = vec![Span::styled(mark, style)];
    let text = toast.text.replace('\n', " ");
    if toast.kind == ToastKind::Error {
        let suffix = format!("  {}", lformat!("(details in the Log tab · any key hides this)"));
        let room = (area.width as usize).saturating_sub(2 + suffix.chars().count());
        spans.push(Span::styled(truncate(&text, room.max(20)), style));
        spans.push(Span::styled(suffix, dim()));
    } else {
        spans.push(Span::styled(truncate(&text, area.width as usize - 2), style));
    }
    frame.render_widget(Line::from(spans), area);
}

/// The footer: what can be done here, from the action registry, fitted to the width.
fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect, hits: &mut Hits) {
    let hints: Vec<(String, String, Option<Action>)> = if app.searching {
        vec![
            (String::from("Enter"), lformat!("open"), None),
            (String::from("Esc"), lformat!("clear"), None),
            (String::from("↑↓"), lformat!("move"), None),
        ]
    } else if let Some(modal) = &app.modal {
        modals::footer(modal)
            .into_iter()
            .map(|(k, l)| (k, l, None))
            .collect()
    } else {
        actions::footer(app, area.width as usize)
            .into_iter()
            .map(|h| (h.key, h.label, Some(h.action)))
            .collect()
    };
    let mut x = area.x;
    for (n, (key, label, action)) in hints.iter().enumerate() {
        if n > 0 {
            x += HINT_GAP as u16;
        }
        let width = (key.chars().count() + 1 + label.chars().count()) as u16;
        if x + width > area.right() {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        frame.render_widget(Line::from(vec![Span::styled(key.clone(), key_style()), Span::styled(format!(" {}", label), dim())]), rect);
        if let Some(action) = action {
            hits.click(rect, Target::Action(*action));
        }
        x += width;
    }
}

/// A row of buttons `[ Save ]  [ Cancel ]`; the focused one is highlighted. Returns their areas.
pub fn button_row(frame: &mut Frame<'_>, area: Rect, labels: &[String], focus: Option<usize>, danger: Option<usize>) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut x = area.x;
    for (n, label) in labels.iter().enumerate() {
        // the focused button is recognisable without colour too
        let text = if focus == Some(n) { format!("▶ {} ◀", label) } else { format!("[ {} ]", label) };
        let width = (text.chars().count() as u16).min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let style = if focus == Some(n) {
            if danger == Some(n) {
                Style::new().fg(Color::Black).bg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD)
            }
        } else if danger == Some(n) {
            Style::new().fg(Color::Red)
        } else {
            Style::new()
        };
        frame.render_widget(Span::styled(text, style), rect);
        rects.push(rect);
        x += width + 2;
    }
    rects
}

/// Width of a button row.
pub fn button_row_width(labels: &[String]) -> u16 {
    labels.iter().map(|l| l.chars().count() as u16 + 4).sum::<u16>() + 2 * labels.len().saturating_sub(1) as u16
}

#[cfg(test)]
mod tests;
