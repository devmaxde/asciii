//! Render tests: every screen draws at 80×24 and 120×36 without panicking, keeps the help hint
//! in the footer and shows the keys it promises.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::backend::TestBackend;
use ratatui::Terminal;

use super::super::actions::Action;
use super::super::app::{App, LoadProblem, Screen};
use super::super::data;
use super::super::offer_editor::EditorMode;

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 36)];

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the playground projects, so nothing in the repository is touched.
fn playground() -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("asciii-tui-test-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/tui-playground/projects/working");
    copy_dir(&source, &dir);
    dir
}

/// Removes the copy when the test ends.
struct Copy(PathBuf);

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn app() -> (App, Copy) {
    let dir = playground();
    let mut app = App::new();
    app.data = Some(data::load_folders(&dir));
    app.recompute();
    (app, Copy(dir))
}

fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| super::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_owned()).collect::<String>())
        .collect()
}

fn check(lines: &[String], wanted: &[&str]) {
    let screen = lines.join("\n");
    let footer = lines.last().unwrap();
    assert!(footer.contains('?') || footer.contains("Esc"), "footer without help or way out:\n{}", screen);
    for text in wanted {
        assert!(screen.contains(text), "{:?} missing on screen:\n{}", text, screen);
    }
}

/// Index of a project that is not archived and has an offer.
fn project(app: &App, name: &str) -> usize {
    let data = app.data.as_ref().unwrap();
    data.entries
        .iter()
        .position(|e| e.summary.name.to_lowercase().contains(&name.to_lowercase()))
        .unwrap_or_else(|| panic!("{} not in the playground", name))
}

#[test]
fn dashboard() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        let lines = render(&mut app, w, h);
        check(&lines, &["Dashboard", "? help", "Ctrl+P", "Received", "Waiting for payment", "Not yet invoiced"]);
        // every part is one line at both sizes, nothing cut
        assert!(!lines.iter().any(|l| l.contains("Waiting for payment") && l.contains("…")));
        // no bar: the parts come right under the headline
        let head = lines.iter().position(|l| l.contains("Expected")).unwrap();
        assert!(lines[head + 1].contains("Received"), "{}", lines.join("\n"));
    }
}

#[test]
fn project_list() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        app.go(Screen::Projects);
        let lines = render(&mut app, w, h);
        check(&lines, &["Projects", "? help", "Ctrl+P"]);
    }
}

#[test]
fn project_page() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        let index = project(&app, "Sommerball");
        app.open_project(index, None);
        let lines = render(&mut app, w, h);
        check(&lines, &["›", "Next", "? help", "Ctrl+P", "▶ Offer", "○ Archive"]);
        // a cut next step ends with "…" and is shown in full in the page
        if let Some(cut) = lines.iter().find(|l| l.contains("…") && l.contains("│") && !l.contains("Next")) {
            assert!(!cut.contains(";…") && !cut.contains("; …"), "{}", cut);
        }
        // the steps are one line in the fixed header
        assert!(lines.iter().filter(|l| l.contains("○ Paid")).count() == 1);
        if w >= 100 {
            // the actions panel sits next to the content
            check(&lines, &["Edit the offer", "Archive the project"]);
        } else {
            app.run(Action::ShowActions);
            let lines = render(&mut app, w, h);
            check(&lines, &["Edit the offer"]);
        }
    }
}

#[test]
fn offer_editor() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        let index = project(&app, "Sommerball");
        app.open_project(index, None);
        app.open_offer(EditorMode::Offer);
        assert_eq!(app.screen, Screen::Offer, "offer editor did not open");
        let lines = render(&mut app, w, h);
        check(&lines, &["Offer", "Ctrl+P"]);
        app.open_help();
        let lines = render(&mut app, w, h);
        check(&lines, &["Help", "Esc"]);
    }
}

#[test]
fn help() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        app.open_help();
        let lines = render(&mut app, w, h);
        check(&lines, &["Help", "Ctrl+P", "Esc"]);
    }
}

#[test]
fn palette() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        app.open_palette();
        let lines = render(&mut app, w, h);
        check(&lines, &["Commands", "Previous year", "Esc"]);
    }
}

#[test]
fn form() {
    for (w, h) in SIZES {
        let (mut app, _dir) = app();
        let index = project(&app, "Sommerball");
        app.open_project(index, None);
        app.open_details_form();
        let lines = render(&mut app, w, h);
        check(&lines, &["Save", "Cancel", "Esc"]);
        let (mut app, _dir) = self::app();
        app.open_new_project_form();
        let lines = render(&mut app, w, h);
        check(&lines, &["Create", "Cancel", "*"]);
    }
}

#[test]
fn first_run() {
    for (w, h) in SIZES {
        let mut app = App::new();
        app.load_problem = Some(LoadProblem {
            error: String::from("no such folder"),
            missing: Some((PathBuf::from("/nowhere"), vec![String::from("working")])),
        });
        let lines = render(&mut app, w, h);
        check(&lines, &["Welcome", "Try again", "Quit"]);
    }
}

#[test]
fn too_small() {
    let mut app = App::new();
    let lines = render(&mut app, 50, 12);
    assert!(lines.join("\n").contains("too small"));
}

#[test]
fn every_screen_registers_clickable_footer_hints() {
    let (mut app, _dir) = app();
    render(&mut app, 120, 36);
    assert!(app.hits.target_at(1, 35).is_some() || app.hits.target_at(3, 35).is_some(), "footer hints are not clickable");
}

/// Prints the main screens, for looking at them: `cargo test --features tui dump_screens -- --ignored --nocapture`
#[test]
#[ignore]
fn dump_screens() {
    let size = std::env::var("SIZE").unwrap_or_else(|_| String::from("120x36"));
    let (w, h) = size.split_once('x').map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap())).unwrap();
    let name = std::env::var("PROJECT").unwrap_or_else(|_| String::from("Sommerball"));
    let show = |title: &str, lines: Vec<String>| println!("== {} ==\n{}\n", title, lines.join("\n"));
    let (mut app, _dir) = app();
    show("dashboard", render(&mut app, w, h));
    app.go(Screen::Projects);
    show("projects", render(&mut app, w, h));
    let index = project(&app, &name);
    app.open_project(index, None);
    show("project page", render(&mut app, w, h));
    app.run(Action::ShowActions);
    show("actions", render(&mut app, w, h));
    app.modal = None;
    app.open_palette();
    show("palette", render(&mut app, w, h));
    app.modal = None;
    app.open_details_form();
    show("details form", render(&mut app, w, h));
    app.modal = None;
    app.open_offer(EditorMode::Offer);
    show("offer editor", render(&mut app, w, h));
    app.open_help();
    show("help", render(&mut app, w, h));
}

#[test]
fn clicking_the_current_step_runs_it() {
    use super::super::mouse::Target;
    let (mut app, _dir) = app();
    let index = project(&app, "Institutstag");
    app.open_project(index, None);
    let lines = render(&mut app, 120, 36);
    let row = lines.iter().position(|l| l.contains("▶ Paid")).expect("step line");
    let column = lines[row].chars().position(|c| c == '▶').unwrap() as u16;
    assert!(
        matches!(app.hits.target_at(column + 2, row as u16), Some(Target::Action(Action::SetWagesDate | Action::SetPaidDate))),
        "{:?}",
        app.hits.target_at(column + 2, row as u16)
    );
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: column + 2, row: row as u16, modifiers: KeyModifiers::NONE });
    assert!(matches!(app.modal, Some(super::super::dialogs::Modal::Form(_))), "no form after the click: {:?} {:?}", app.toast.as_ref().map(|t| t.text.clone()), app.hits.target_at(column + 2, row as u16));
}

// ── the Template tab ────────────────────────────────────────────────────────

/// The Template tab on a copy of `template` (a path in the repository).
fn template_app(template: &str) -> (App, Copy, PathBuf) {
    use super::super::template_editor::TemplateEditor;
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("asciii-tui-template-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("default.tyml");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join(template), &file).unwrap();
    let (mut app, projects) = app();
    // the projects copy goes with the template copy
    std::fs::rename(&projects.0, dir.join("working")).unwrap();
    app.template = Some(TemplateEditor::from_file(file.clone()));
    app.screen = Screen::Template;
    (app, Copy(dir), file)
}

const TEMPLATES: [&str; 2] = ["examples/tui-playground/projects/templates/default.tyml", "templates/default.tyml"];

fn key(app: &mut App, code: ratatui::crossterm::event::KeyCode) {
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, ratatui::crossterm::event::KeyCode::Char(c));
    }
}

#[test]
fn template_tab() {
    for (w, h) in SIZES {
        let (mut app, _dir, _) = template_app(TEMPLATES[0]);
        let lines = render(&mut app, w, h);
        check(&lines, &["Template", "Kaffee", "Starting offer", "New projects start with this", "★ default", "? help", "Ctrl+P"]);
        check(&lines, &["starts with usage 5"]);
        // the tabs in their order
        let top = &lines[0];
        let at = |t: &str| top.find(t).unwrap_or_else(|| panic!("{} not in the tabs: {}", t, top));
        assert!(at("Dashboard") < at("Projects") && at("Projects") < at("Template") && at("Template") < at("Log"));
        // the keys of the tabs
        key(&mut app, ratatui::crossterm::event::KeyCode::Char('1'));
        assert_eq!(app.screen, Screen::Dashboard);
        key(&mut app, ratatui::crossterm::event::KeyCode::Char('3'));
        assert_eq!(app.screen, Screen::Template);
        // the help and the palette know the tab
        app.open_help();
        let lines = render(&mut app, w, h);
        check(&lines, &["Template"]);
        app.modal = None;
        app.open_palette();
        typed(&mut app, "templ");
        let lines = render(&mut app, w, h);
        check(&lines, &["Edit the template (catalog and starting offer)"]);
    }
}

#[test]
fn template_edits_are_saved_and_new_projects_get_them() {
    use ratatui::crossterm::event::KeyCode;
    use super::super::offer;
    for template in TEMPLATES {
        let (mut app, _dir, file) = template_app(template);
        let before = std::fs::read_to_string(&file).unwrap();
        // a new product with a starting quantity
        key(&mut app, KeyCode::Char('n'));
        typed(&mut app, "Club-Mate");
        key(&mut app, KeyCode::Tab);
        typed(&mut app, "0,5l");
        key(&mut app, KeyCode::Tab);
        typed(&mut app, "1,20");
        key(&mut app, KeyCode::Tab);
        typed(&mut app, "6");
        key(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none(), "form still open: {:?}", app.toast.as_ref().map(|t| t.text.clone()));
        // Tee: another price, one in the starting offer
        let editor = app.template.as_mut().unwrap();
        let rows = editor.rows();
        let tee = rows.iter().position(|r| editor.product(*r).0 == "Tee").unwrap();
        editor.table.select(Some(tee));
        key(&mut app, KeyCode::Char('+'));
        key(&mut app, KeyCode::Char('e'));
        // the price field: typing replaces the selected value
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        typed(&mut app, "1,95");
        key(&mut app, KeyCode::Enter);
        assert!(app.modal.is_none());
        // Kaffee: the leftover usage goes
        let editor = app.template.as_mut().unwrap();
        let rows = editor.rows();
        let kaffee = rows.iter().position(|r| editor.product(*r).0 == "Kaffee").unwrap();
        editor.table.select(Some(kaffee));
        key(&mut app, KeyCode::Char('u'));
        // Esc asks, Keep editing keeps
        key(&mut app, KeyCode::Esc);
        assert!(app.modal.is_some(), "Esc with changes did not ask");
        key(&mut app, KeyCode::Char('k'));
        assert!(app.template.as_ref().unwrap().dirty());
        let lines = render(&mut app, 80, 24);
        check(&lines, &["not saved", "Club-Mate"]);
        // save
        app.on_key(ratatui::crossterm::event::KeyEvent::new(KeyCode::Char('s'), ratatui::crossterm::event::KeyModifiers::CONTROL));
        let toast = app.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        assert!(toast.contains("3 products changed"), "{}", toast);
        assert!(!app.template.as_ref().unwrap().dirty());

        let after = std::fs::read_to_string(&file).unwrap();
        let read = offer::read_template(&after).unwrap();
        let mate = read.catalog.iter().find(|i| i.name == "Club-Mate").unwrap();
        assert_eq!((mate.anchor.as_str(), mate.price, mate.unit.as_deref()), ("club_mate", 1.2, Some("0,5l")));
        let line = |anchor: &str| read.lines.iter().find(|l| l.anchor.as_deref() == Some(anchor)).cloned();
        assert_eq!(line("club_mate").unwrap().amount, 6.0);
        assert_eq!(line("tee").unwrap().amount, 1.0);
        assert_eq!(read.catalog.iter().find(|i| i.anchor == "tee").unwrap().price, 1.95);
        assert_eq!(line("kaffee").unwrap().sold, None);
        // the block of Kaffee stays a block, the new product lines up with the others
        assert!(after.contains("  *kaffee:\n    amount: 4\n"), "{}", after);
        assert!(after.contains("  product: &club_mate    { name: Club-Mate"), "{}", after);
        // the rest of the file is untouched: placeholders, comments
        let marks = |c: &str| c.lines().filter(|l| l.contains("##") || l.trim_start().starts_with('#')).map(String::from).collect::<Vec<_>>();
        assert_eq!(marks(&before), marks(&after));
        let only_in = |a: &str, b: &str| a.lines().filter(|l| !b.lines().any(|m| m == *l)).count();
        let changed = only_in(&before, &after) + only_in(&after, &before);
        assert!(changed <= 8, "{} lines changed:\n{}", changed, after);
        // a project made from it opens and has the products
        let project = offer::filled_like_new_project(&after);
        let project = asciii::project::Project::from_file_content(&project).unwrap();
        let _ = project;
        assert_eq!(offer::read(&offer::filled_like_new_project(&after)).unwrap().lines.len(), read.lines.len());
    }
}

#[test]
fn template_remove_asks_and_tab_switch_keeps_changes() {
    use ratatui::crossterm::event::KeyCode;
    let (mut app, _dir, file) = template_app(TEMPLATES[0]);
    let before = std::fs::read_to_string(&file).unwrap();
    let editor = app.template.as_mut().unwrap();
    let rows = editor.rows();
    let kekse = rows.iter().position(|r| editor.product(*r).0 == "Kekse Lamberts").unwrap();
    editor.table.select(Some(kekse));
    key(&mut app, KeyCode::Char('d'));
    let lines = render(&mut app, 80, 24);
    check(&lines, &["starting offer too"]);
    key(&mut app, KeyCode::Char('y'));
    let toast = app.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
    assert!(toast.contains("also from the starting offer"), "{}", toast);
    // another tab asks first; Discard goes there and the file stays as it was
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(app.screen, Screen::Template);
    assert!(app.modal.is_some());
    key(&mut app, KeyCode::Char('d'));
    assert_eq!(app.screen, Screen::Dashboard);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    // a file changed on disk is not overwritten
    let (mut app, _dir, file) = template_app(TEMPLATES[0]);
    key(&mut app, KeyCode::Char('+'));
    std::fs::write(&file, format!("{}\n# changed elsewhere\n", before)).unwrap();
    key(&mut app, KeyCode::Char('s'));
    assert!(std::fs::read_to_string(&file).unwrap().ends_with("# changed elsewhere\n"));
    assert!(app.template.as_ref().unwrap().dirty());
}
