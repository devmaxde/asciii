//! Shown instead of the dashboard while loading, and when the projects could not be loaded:
//! what was expected where, what is wrong, how to fix it.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use asciii::CONFIG;

use super::super::app::App;
use super::super::mouse::{Hits, Target};
use super::super::paths;
use super::{bold, button_row, centered, dim, panel, wrap, ACCENT};

pub fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect, hits: &mut Hits) {
    let Some(problem) = app.load_problem.clone() else {
        let text = if app.is_loading() { lformat!("loading projects …") } else { String::new() };
        frame.render_widget(Paragraph::new(text).centered(), centered(area, 40, 1));
        return;
    };

    let storage = paths::storage().map(paths::tilde).unwrap_or_else(|| lformat!("(unknown)"));
    let local = std::path::Path::new(asciii::config::DEFAULT_LOCATION);
    let config = if local.exists() {
        lformat!("{} in this folder", asciii::config::DEFAULT_LOCATION)
    } else if CONFIG.path.exists() {
        paths::tilde(&CONFIG.path)
    } else {
        lformat!("the defaults ({} does not exist)", paths::tilde(&CONFIG.path))
    };
    let folders = ["dirs/working", "dirs/archive", "dirs/templates"]
        .iter()
        .map(|k| format!("{}/", CONFIG.get_str_or(k).unwrap_or_default()))
        .collect::<Vec<_>>()
        .join(", ");

    let mut lines = vec![
        Line::from(Span::styled(lformat!("asciii cannot open the project folder yet."), bold().fg(Color::Yellow))),
        Line::default(),
        Line::from(vec![Span::styled(format!("{:<14}", lformat!("Expected in")), dim()), Span::styled(storage, bold())]),
        Line::from(vec![Span::styled(format!("{:<14}", lformat!("with")), dim()), Span::raw(folders)]),
        Line::from(vec![Span::styled(format!("{:<14}", lformat!("set by")), dim()), Span::raw(lformat!("path and dirs/storage in {}", config))]),
    ];
    match &problem.missing {
        Some((_, missing)) => lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", lformat!("Missing")), dim()),
            Span::styled(missing.join(", "), Style::new().fg(Color::Red)),
        ])),
        None => lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", lformat!("Problem")), dim()),
            Span::styled(problem.error.replace('\n', " "), Style::new().fg(Color::Red)),
        ])),
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(lformat!("How to fix it"), Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(lformat!("• Get the shared projects: in a normal terminal run  asciii bootstrap <repository-url>")));
    lines.push(Line::from(lformat!("• Or point the configuration to the right folder: set path and dirs/storage in ~/.asciii.yml")));
    if problem.missing.is_some() {
        lines.push(Line::from(lformat!("• Or start empty here: \"Create empty project folder\" below")));
    }
    lines.push(Line::from(Span::styled(lformat!("Then choose \"Try again\". The Log tab has the details."), dim())));

    let buttons = app.first_run_buttons();
    let labels: Vec<String> = buttons.iter().map(|(label, _)| label.clone()).collect();
    let width = area.width.saturating_sub(4).min(96);
    let lines = wrap(lines, width.saturating_sub(2) as usize);
    let text_height = lines.len() as u16 + 2;
    let rect = centered(area, width, (text_height + 3).min(area.height));
    let block = panel(lformat!("Welcome to asciii"));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [text_area, buttons_area] = Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(inner);
    frame.render_widget(Paragraph::new(lines), text_area);
    app.first_run_focus = app.first_run_focus.min(labels.len().saturating_sub(1));
    let row = Rect { y: buttons_area.y + 1, height: 1, ..buttons_area };
    for (n, rect) in button_row(frame, row, &labels, Some(app.first_run_focus), None).into_iter().enumerate() {
        hits.click(rect, Target::Button(n));
    }
}
