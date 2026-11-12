//! The command palette (`Ctrl+P`, `F1`, `:`): every command of the current screen and every project, by name.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::actions::{Action, Avail, Ctx};
use super::app::App;
use super::data::Location;
use super::dialogs::Modal;
use super::text_input::TextInput;

#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub input: TextInput,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Action(Action),
    /// index into `data.entries`
    Project(usize),
}

#[derive(Debug, Clone)]
pub struct Row {
    pub item: Item,
    pub label: String,
    /// shortcut, right-aligned
    pub keys: String,
    /// why it cannot run now
    pub disabled: Option<String>,
}

/// How well `label` matches `query`: `Some(0)` word start, `Some(1)` anywhere, `Some(2)` letters in order.
pub fn score(label: &str, query: &str) -> Option<u8> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let label = label.to_lowercase();
    if let Some(at) = label.find(&query) {
        let word_start = at == 0 || !label[..at].chars().last().is_some_and(char::is_alphanumeric);
        return Some(if word_start { 0 } else { 1 });
    }
    let mut chars = label.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|q| chars.any(|c| c == q))
        .then_some(2)
}

/// Most projects listed for one query.
const PROJECTS: usize = 30;

/// What the palette offers for `query`: matching commands and projects. Close matches come first,
/// available commands before greyed-out ones, loose matches (letters in order) last.
pub fn rows(app: &App, query: &str) -> Vec<Row> {
    let ctx = app.ctx();
    // (loose match, disabled, score, project, row)
    let mut found: Vec<(bool, bool, u8, bool, Row)> = Action::all()
        .filter(|a| a.in_palette())
        .filter_map(|action| {
            let disabled = match action.availability(app) {
                // project commands found from elsewhere say where they work
                Avail::Hidden if !query.trim().is_empty() && app.data.is_some() && action.in_ctx(Ctx::Detail) && !action.in_ctx(ctx) => {
                    Some(lformat!("on the project page: open a project first"))
                },
                Avail::Hidden => return None,
                Avail::Enabled => None,
                Avail::Disabled(reason) => Some(reason),
            };
            let label = action.title(app);
            let score = score(&label, query).or_else(|| score(&action.label(), query))?;
            let keys = action.keys(ctx).first().map(|k| k.label()).unwrap_or_default();
            Some((score == 2, disabled.is_some(), score, false, Row {
                item: Item::Action(action),
                label,
                keys,
                disabled,
            }))
        })
        .collect();

    if !query.trim().is_empty() {
        if let Some(data) = &app.data {
            let mut projects: Vec<(u8, usize)> = data
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| score(&e.summary.name, query).map(|s| (s, i)))
                .collect();
            projects.sort_by_key(|(s, i)| (*s, data.entries[*i].location != Location::Working, *i));
            found.extend(projects.into_iter().take(PROJECTS).map(|(score, i)| {
                let entry = &data.entries[i];
                let label = match entry.location {
                    Location::Working => lformat!("Open {}", entry.summary.name),
                    Location::Archive(year) => lformat!("Open {} (archive {})", entry.summary.name, year),
                };
                (score == 2, false, score, true, Row {
                    item: Item::Project(i),
                    label,
                    keys: String::new(),
                    disabled: None,
                })
            }));
        }
    }
    // stable: commands keep the registry order, projects their own
    found.sort_by_key(|(loose, disabled, score, project, _)| (*loose, *disabled, *score, *project));
    found.into_iter().map(|(_, _, _, _, row)| row).collect()
}

impl App {
    pub fn open_palette(&mut self) {
        self.modal = Some(Modal::Palette(Palette::default()));
    }

    pub fn on_palette_key(&mut self, mut palette: Palette, key: &KeyEvent) -> Option<Modal> {
        let count = rows(self, palette.input.text()).len();
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => return self.run_palette_row(palette, None),
            KeyCode::Down | KeyCode::Tab if count > 0 => palette.selected = (palette.selected + 1).min(count - 1),
            KeyCode::Up | KeyCode::BackTab => palette.selected = palette.selected.saturating_sub(1),
            KeyCode::PageDown if count > 0 => palette.selected = (palette.selected + 10).min(count - 1),
            KeyCode::PageUp => palette.selected = palette.selected.saturating_sub(10),
            _ => {
                if palette.input.on_key(key) {
                    palette.selected = 0;
                }
            },
        }
        Some(Modal::Palette(palette))
    }

    /// Runs row `index` (the selected one if `None`). Disabled rows keep the palette open and say why.
    pub fn run_palette_row(&mut self, mut palette: Palette, index: Option<usize>) -> Option<Modal> {
        let rows = rows(self, palette.input.text());
        let index = index.unwrap_or(palette.selected);
        let Some(row) = rows.get(index) else { return Some(Modal::Palette(palette)) };
        if let Some(reason) = &row.disabled {
            palette.selected = index;
            self.notify(lformat!("{}: {}", row.label, reason));
            return Some(Modal::Palette(palette));
        }
        match row.item {
            Item::Action(action) => self.run(action),
            Item::Project(index) => self.open_project(index, None),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_word_starts_before_substrings_before_letters() {
        assert_eq!(score("Create the invoice PDF", "invoice"), Some(0));
        assert_eq!(score("Create the invoice PDF", "voice"), Some(1));
        assert_eq!(score("Create the invoice PDF", "cipdf"), Some(2));
        assert_eq!(score("Create the invoice PDF", "xyz"), None);
        assert_eq!(score("Sommerball", "SOMMER"), Some(0));
        assert_eq!(score("anything", ""), Some(0));
    }
}
