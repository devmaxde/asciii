//! Interactive terminal UI (`asciii tui`), only built with the `tui` feature.
//!
//! The CLI does not depend on anything in here.

use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Error;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use ratatui::DefaultTerminal;

pub mod actions;
pub mod app;
pub mod checks;
pub mod commands;
pub mod data;
pub mod dialogs;
pub mod forms;
pub mod input;
pub mod lifecycle;
pub mod logger;
pub mod mouse;
pub mod offer;
pub mod offer_editor;
pub mod ops;
pub mod palette;
pub mod paths;
pub mod template_editor;
pub mod text_input;
pub mod ui;
pub mod yaml_edit;

use self::app::App;
use self::dialogs::{Button, Dialog, Effect, Modal, Tone};
use self::ops::Outcome;

/// Mouse reporting wanted by the user (palette: "Mouse: on/off"); read when the terminal is taken back.
static MOUSE: AtomicBool = AtomicBool::new(true);

pub fn mouse_wanted() -> bool {
    MOUSE.load(Ordering::Relaxed)
}

/// Switches mouse reporting of the terminal on or off.
pub fn set_mouse_capture(on: bool) -> io::Result<()> {
    if on {
        execute!(stdout(), EnableMouseCapture)
    } else {
        execute!(stdout(), DisableMouseCapture)
    }
}

/// Puts the terminal back as it was: no mouse reporting, no bracketed paste, normal screen.
fn restore() {
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
}

/// Entry point of `asciii tui`.
pub fn run() -> Result<(), Error> {
    logger::start_recording();
    let mut terminal = ratatui::try_init()?;
    // ratatui's panic hook restores the screen; mouse reporting has to go off before that
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
        hook(info);
    }));
    let _ = execute!(stdout(), EnableBracketedPaste);
    let _ = set_mouse_capture(true);
    logger::set_screen(true);
    let result = event_loop(&mut terminal);
    logger::set_screen(false);
    restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal) -> Result<(), Error> {
    let mut app = App::new();
    app.start_loading(None);

    while !app.quit {
        app.poll_loader();
        app.expire_toast();
        if app.mouse != mouse_wanted() {
            MOUSE.store(app.mouse, Ordering::Relaxed);
            set_mouse_capture(app.mouse)?;
        }
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        if let Some(op) = app.pending.take() {
            run_op(&mut app, op, terminal)?;
            continue;
        }

        if event::poll(Duration::from_millis(200))? {
            app.on_event(event::read()?);
        }
    }
    Ok(())
}

/// Runs an action that needs the terminal or writes files, and shows its result.
fn run_op(app: &mut App, op: ops::Op, terminal: &mut DefaultTerminal) -> Result<(), Error> {
    let reload = op.changes_data();
    let remote = op.touches_remote();
    let select = op.file_to_select();
    let saves_offer = matches!(op, ops::Op::SaveOffer { .. });
    let result = ops::execute(op, terminal);
    // the template file may have been edited
    app.refresh_template();
    // a saved offer closes its editor, a failed save keeps the changes on screen
    if saves_offer {
        if result.is_ok() {
            app.close_offer();
            if app.quit_after_save {
                app.quit = true;
            }
        } else {
            app.quit_after_save = false;
        }
    }
    match result {
        Ok(Outcome::Message(message)) => app.success(message),
        Ok(Outcome::Output { title, text }) => {
            app.modal = Some(Modal::Output { title, text, scroll: 0 });
        },
        Ok(Outcome::CatalogSync { message, template, changes }) => {
            app.success(message);
            app.ask_catalog_sync(template, changes);
        },
        Ok(Outcome::File { title, message, path }) => {
            log::info!("{}", message);
            let folder = path.parent().map(ToOwned::to_owned).unwrap_or_else(|| path.clone());
            app.show(Modal::Dialog(Dialog::with(
                title,
                vec![message],
                Tone::Success,
                vec![
                    Button::new(lformat!("Open"), Some('o'), Effect::OpenPath(path)),
                    Button::new(lformat!("Open folder"), Some('f'), Effect::OpenPath(folder)),
                    Button::new(lformat!("Close"), Some('c'), Effect::Close),
                ],
                2,
                2,
            )));
        },
        Ok(Outcome::Created { message, file }) => {
            app.success(message);
            app.open_after_load(file);
        },
        Err(error) if error.is::<ops::ScreenLost>() => return Err(error),
        Err(error) => app.fail(format!("{:#}", error)),
    }
    if reload {
        app.start_loading(select);
    }
    if remote {
        app.check_remote();
    }
    Ok(())
}
