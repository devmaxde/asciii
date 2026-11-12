//! The log tab: wrapped lines, newest at the bottom, with a scrollbar and the position.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::super::app::App;
use super::super::logger;
use super::super::mouse::{Hits, Wheel};
use super::{panel, scrollbar, wrap};

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let width = area.width.saturating_sub(3) as usize;
    let lines: Vec<Line<'static>> = logger::lines()
        .into_iter()
        .flat_map(|line| {
            let color = match line.level {
                log::Level::Error => Color::Red,
                log::Level::Warn => Color::Yellow,
                log::Level::Info => Color::Green,
                _ => Color::DarkGray,
            };
            let prefix = format!("{} {:<5} ", line.time.format("%H:%M:%S"), line.level);
            let lines: Vec<Line<'static>> = line
                .message
                .lines()
                .enumerate()
                .map(|(n, text)| {
                    let lead = if n == 0 { prefix.clone() } else { " ".repeat(prefix.len()) };
                    Line::from(vec![Span::styled(lead, Style::new().fg(color)), Span::raw(text.to_owned())])
                })
                .collect();
            // continuation lines line up after the time and level
            wrap(lines, width)
                .into_iter()
                .map(|l| if l.spans.first().is_some_and(|s| s.content.trim().is_empty() && l.spans.len() > 1) {
                    let mut spans = l.spans;
                    spans[0] = Span::raw(" ".repeat(prefix.len()));
                    Line::from(spans)
                } else {
                    l
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let visible = area.height.saturating_sub(2) as usize;
    let total = lines.len();
    let max_scroll = total.saturating_sub(visible) as u16;
    app.log_scroll = app.log_scroll.min(max_scroll);
    let top = max_scroll - app.log_scroll;
    let title = if total > visible {
        lformat!("Log · lines {}–{} of {} · ↑↓ PgUp PgDn, End newest", top as usize + 1, (top as usize + visible).min(total), total)
    } else if total == 0 {
        lformat!("Log · empty")
    } else {
        lformat!("Log · {} lines", total)
    };
    frame.render_widget(Paragraph::new(lines).block(panel(title)).scroll((top, 0)), area);
    hits.wheel(area, Wheel::Log);
    scrollbar(frame, area, total, visible, top as usize);
}
