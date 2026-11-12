//! Actions that change files, call git or need the real terminal.
//!
//! Everything goes through the existing library API; nothing here knows about the project file format.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, stdout, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{bail, format_err, Error};
use ratatui::crossterm::{
    execute,
    terminal::{enable_raw_mode, Clear, ClearType, EnterAlternateScreen},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::{DefaultTerminal, Terminal};

use asciii::project::{spec::Redeemable, BillType, Exportable, Project};
use asciii::storage::{self, Storable, Storage, StorageDir};
use asciii::{actions, util, CONFIG};

#[cfg(feature = "document_export")]
use asciii::document_export::{self, error::ExportError, ExportConfig};
#[cfg(feature = "document_export")]
use asciii::storage::StorageSelection;

use super::logger;
use super::paths;
use super::offer;
use super::yaml_edit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOp {
    Status,
    Diff,
    Log,
    AddAll,
    Commit,
    Pull,
    Push,
}

impl GitOp {
    pub const ALL: [GitOp; 7] = [
        GitOp::Status,
        GitOp::Diff,
        GitOp::Log,
        GitOp::AddAll,
        GitOp::Commit,
        GitOp::Pull,
        GitOp::Push,
    ];

    pub fn label(self) -> String {
        match self {
            GitOp::Status => lformat!("status: show changed files"),
            GitOp::Diff => lformat!("diff: show changes"),
            GitOp::Log => lformat!("log: show history"),
            GitOp::AddAll => lformat!("add: stage all changes"),
            GitOp::Commit => lformat!("commit: save staged changes"),
            GitOp::Pull => lformat!("pull: get changes from remote"),
            GitOp::Push => lformat!("push: upload changes to remote"),
        }
    }
}

/// An action requested by the UI, executed by the main loop.
#[derive(Debug, Clone)]
pub enum Op {
    Edit(PathBuf),
    /// project fields (path, value), written in one go
    FillFields { file: PathBuf, values: Vec<(String, String)> },
    SetInvoiceNumber { file: PathBuf, number: u32 },
    Make { file: PathBuf, bill_type: Option<BillType>, force: bool },
    Archive { dir: PathBuf, year: i32 },
    Unarchive { dir: PathBuf },
    Delete { dir: PathBuf },
    NewProject { name: String, template: String, fill: Vec<(&'static str, String)>, edit: bool },
    ExportCsv { year: i32 },
    ExportIcs { year: i32 },
    Git(GitOp),
    /// the offer editor's result: the whole project file, already checked
    SaveOffer { file: PathBuf, original: String, content: String, sync: Option<(PathBuf, Vec<offer::Change>)> },
    /// catalog entries to add to or change in a template
    SyncTemplate { template: PathBuf, items: Vec<offer::CatalogItem> },
    /// `offer/accepted: true`, or removing it
    SetAccepted { file: PathBuf, accepted: bool },
}

/// What to show after an action.
pub enum Outcome {
    /// one line in the status bar
    Message(String),
    /// longer output in a scrollable window
    Output { title: String, text: String },
    /// the catalog of the project changed: ask whether the template should get the changes too
    CatalogSync { message: String, template: PathBuf, changes: Vec<offer::Change> },
    /// a file was written (PDF, CSV, calendar): offer to open it
    File { title: String, message: String, path: PathBuf },
    /// a project was created: show its page
    Created { message: String, file: PathBuf },
}

impl Op {
    /// Whether the projects must be reloaded afterwards.
    pub fn changes_data(&self) -> bool {
        !matches!(
            self,
            Op::ExportCsv { .. }
                | Op::ExportIcs { .. }
                | Op::Git(GitOp::Status)
                | Op::Git(GitOp::Diff)
                | Op::Git(GitOp::Log)
        )
    }

    /// Whether the comparison with the git remote is outdated afterwards.
    pub fn touches_remote(&self) -> bool {
        matches!(self, Op::Git(GitOp::Pull) | Op::Git(GitOp::Push) | Op::Git(GitOp::Commit))
    }

    /// The file to select again after reloading.
    pub fn file_to_select(&self) -> Option<PathBuf> {
        match self {
            Op::Edit(file)
            | Op::FillFields { file, .. }
            | Op::SetInvoiceNumber { file, .. }
            | Op::Make { file, .. }
            | Op::SaveOffer { file, .. }
            | Op::SetAccepted { file, .. } => {
                Some(file.clone())
            },
            _ => None,
        }
    }
}

/// Leaves the TUI, runs `f` on the normal terminal, waits for Enter if asked to, comes back.
fn suspended<R>(terminal: &mut DefaultTerminal, wait: bool, f: impl FnOnce() -> R) -> Result<R, Error> {
    logger::set_screen(false);
    // the other program gets a normal terminal, without mouse reporting
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();

    let result = f();

    if wait {
        print!("\n{} ", lformat!("Press Enter to return to asciii."));
        let _ = stdout().flush();
        let _ = io::stdin().lock().read_line(&mut String::new());
    }

    // No `terminal.clear()`: it asks the terminal for the cursor position, which can time out right after
    // another program used the terminal. A fresh `Terminal` has empty buffers and redraws everything.
    let back = (|| -> io::Result<DefaultTerminal> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, Clear(ClearType::All), EnableBracketedPaste)?;
        super::set_mouse_capture(super::mouse_wanted())?;
        Terminal::new(CrosstermBackend::new(stdout()))
    })();
    match back {
        Ok(fresh) => {
            *terminal = fresh;
            logger::set_screen(true);
            Ok(result)
        },
        Err(error) => Err(ScreenLost(error).into()),
    }
}

/// The terminal could not be taken back after running an external program. The TUI has to stop.
#[derive(Debug)]
pub struct ScreenLost(pub io::Error);

impl std::fmt::Display for ScreenLost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", lformat!("could not return to the terminal interface: {}", self.0))
    }
}

impl std::error::Error for ScreenLost {}

pub fn editor() -> String {
    CONFIG
        .get_str_or("user/editor")
        .map(ToOwned::to_owned)
        .filter(|e| !e.trim().is_empty())
        .or_else(|| env::var("VISUAL").ok())
        .or_else(|| env::var("EDITOR").ok())
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| String::from("vi"))
}

fn open_storage() -> Result<Storage<Project>, Error> {
    storage::setup_with_git::<Project>().or_else(|_| storage::setup::<Project>())
}

fn output_dir() -> PathBuf {
    util::get_valid_path(CONFIG.get_str("output_path"))
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn git_captured(root: &Path, args: &[&str]) -> Result<String, Error> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["--no-pager", "-c", "color.ui=never"])
        .args(args)
        .output()
        .map_err(|e| format_err!("{}", lformat!("failed to run git: {}", e)))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if text.trim().is_empty() {
        text = lformat!("(no output)");
    }
    Ok(text)
}

/// Where `x` / `c` write the export of `year`.
pub fn export_path(year: i32, ics: bool) -> PathBuf {
    output_dir().join(format!("asciii-{}.{}", year, if ics { "ics" } else { "csv" }))
}

/// Creates the folders of the storage from the configuration and a default template, for the start page.
/// Existing folders and files are left alone.
pub fn create_storage() -> Result<String, Error> {
    let get = |key: &str| {
        CONFIG
            .get_str_or(key)
            .map(ToOwned::to_owned)
            .ok_or_else(|| format_err!("{}", lformat!("{} is not set in the configuration", key)))
    };
    let (working, archive, templates) = (get("dirs/working")?, get("dirs/archive")?, get("dirs/templates")?);
    let root = storage::get_storage_path();
    let storage = Storage::<Project>::try_new(&root, &working, &archive, &templates)?;
    storage.create_dirs()?;
    let template = storage.templates_dir().join(format!("default.{}", CONFIG.get_str("extensions/project_template")));
    if storage.list_template_names().unwrap_or_default().is_empty() && !template.exists() {
        fs::write(&template, DEFAULT_TEMPLATE)?;
    }
    Ok(lformat!("created {}", paths::tilde(&root)))
}

/// The template of this repository, for a new, empty storage.
const DEFAULT_TEMPLATE: &str = include_str!("../../../templates/default.tyml");

/// Runs `op`. `terminal` is needed to hand the screen to editors, pdflatex and git.
pub fn execute(op: Op, terminal: &mut DefaultTerminal) -> Result<Outcome, Error> {
    match op {
        Op::Edit(file) => {
            let catalog = |file: &Path| fs::read_to_string(file).ok().and_then(|c| offer::read_catalog(&c).ok());
            let before = catalog(&file);
            let editor = editor();
            suspended(terminal, false, || util::pass_to_command(Some(&editor), &[&file]))??;
            let message = lformat!("edited {}", paths::short(&file));
            // products added or prices changed in the editor: maybe the template wants them too
            if let (Some(before), Some(after)) = (before, catalog(&file)) {
                let content = fs::read_to_string(&file).unwrap_or_default();
                if let Some((template, template_catalog)) = offer::template_catalog(&content) {
                    let changes = offer::changes(&before, &after, &template_catalog);
                    if !changes.is_empty() {
                        return Ok(Outcome::CatalogSync { message, template, changes });
                    }
                }
            }
            Ok(Outcome::Message(message))
        },

        Op::SaveOffer { file, original, content, sync } => {
            if fs::read_to_string(&file)? != original {
                bail!(lformat!(
                    "{} was changed by someone else while editing the offer, nothing saved",
                    paths::short(&file)
                ));
            }
            fs::write(&file, &content)?;
            let message = lformat!("saved in {}", paths::short(&file));
            match sync {
                Some((template, changes)) if !changes.is_empty() => Ok(Outcome::CatalogSync { message, template, changes }),
                _ => Ok(Outcome::Message(message)),
            }
        },

        Op::SetAccepted { file, accepted } => {
            let content = fs::read_to_string(&file)?;
            let changed = if accepted {
                yaml_edit::set_or_add(&content, "offer/accepted", "true")?
            } else {
                yaml_edit::remove_key(&content, "offer/accepted")?
            };
            fs::write(&file, changed)?;
            Ok(Outcome::Message(if accepted {
                lformat!("offer marked as accepted")
            } else {
                lformat!("offer no longer marked as accepted")
            }))
        },

        Op::SyncTemplate { template, items } => {
            let content = fs::read_to_string(&template)?;
            fs::write(&template, offer::update_template(&content, &items)?)?;
            Ok(Outcome::Message(lformat!(
                "{} catalog entries written to the template {}",
                items.len(),
                paths::short(&template)
            )))
        },

        Op::FillFields { file, values } => {
            // based on the YAML structure: works with or without the template's `##PLACEHOLDER##` marks;
            // every value is checked by `set_value`, the file is written once, only if all of them worked
            let mut content = fs::read_to_string(&file)?;
            for (field, value) in &values {
                content = yaml_edit::set_value(&content, field, value)?;
            }
            fs::write(&file, content)?;
            Ok(Outcome::Message(match values.as_slice() {
                [(field, value)] => lformat!("{}: {}", yaml_edit::label(field), value),
                _ => lformat!(
                    "saved: {}",
                    values.iter().map(|(f, _)| yaml_edit::label(f)).collect::<Vec<_>>().join(", ")
                ),
            }))
        },

        Op::SetInvoiceNumber { file, number } => {
            let content = fs::read_to_string(&file)?;
            if yaml_edit::value_at(&content, "invoice/number").is_some() {
                bail!(lformat!("Invoice number already set in {}", Project::open(&file)?.short_desc()));
            }
            fs::write(&file, yaml_edit::set_value(&content, "invoice/number", &number.to_string())?)?;
            Ok(Outcome::Message(lformat!("invoice number {} assigned", number)))
        },

        #[cfg(feature = "document_export")]
        Op::Make { file, bill_type, force } => {
            let project = Project::open(&file)?;
            // `export()` would panic on a broken bill, check first
            project.bills()?;
            // export would stop at the same check, but only after the terminal was handed over
            #[cfg(feature = "zugferd")]
            if bill_type == Some(BillType::Invoice) && asciii::zugferd::enabled() {
                let missing = asciii::zugferd::missing(&project);
                if !missing.is_empty() {
                    return Err(asciii::zugferd::ZugferdError::Incomplete(missing.join(", ")).into());
                }
            }
            let target = bill_type.and_then(|t| project.output_file(t));
            let existed = target.as_ref().is_some_and(|t| t.exists());
            // a little slack for file systems with coarse timestamps
            let started = SystemTime::now() - Duration::from_secs(2);
            let config = ExportConfig {
                select: StorageSelection::Paths(vec![file]),
                template_name: CONFIG.get_str("document_export/default_template"),
                bill_type,
                output: None,
                dry_run: false,
                pdf_only: false,
                force,
                print_only: false,
                open: false,
            };
            // without a keyboard pdflatex aborts on errors instead of waiting at its `?` prompt
            suspended(terminal, true, || without_stdin(|| document_export::projects_to_doc(&config)))??
                .map_err(|error| {
                    let tool = CONFIG.get_str("document_export/convert_tool");
                    match error.downcast_ref::<ExportError>() {
                        Some(ExportError::NoPdfCreated) => {
                            format_err!("{}", lformat!("no PDF was created, see the log and the {} output", tool))
                        },
                        _ => error,
                    }
                })?;

            let Some(pdf) = target else {
                return Ok(Outcome::Message(lformat!(
                    "document export finished, output folder: {}",
                    paths::short(&output_dir())
                )));
            };
            let fresh = fs::metadata(&pdf)
                .and_then(|m| m.modified())
                .map(|modified| modified >= started)
                .unwrap_or(false);
            // the ZUGFeRD XML is written next to the PDF when it was attached
            let e_invoice = fs::metadata(pdf.with_extension("xml"))
                .and_then(|m| m.modified())
                .map(|modified| modified >= started)
                .unwrap_or(false);
            let title = lformat!("PDF created");
            if fresh && e_invoice {
                Ok(Outcome::File {
                    title,
                    message: lformat!("created {} with the ZUGFeRD e-invoice attached", paths::short(&pdf)),
                    path: pdf,
                })
            } else if fresh {
                Ok(Outcome::File {
                    title,
                    message: lformat!("created {}", paths::short(&pdf)),
                    path: pdf,
                })
            } else if existed && !force {
                Ok(Outcome::File {
                    title: lformat!("PDF is up to date"),
                    message: lformat!("{} is up to date, nothing rebuilt", paths::short(&pdf)),
                    path: pdf,
                })
            } else {
                bail!(lformat!(
                    "no PDF was created, see the log and the {} output",
                    CONFIG.get_str("document_export/convert_tool")
                ))
            }
        },

        #[cfg(not(feature = "document_export"))]
        Op::Make { .. } => bail!(lformat!("document export is not part of this build")),

        Op::Archive { dir, year } => {
            let storage = open_storage()?;
            let project = Project::open_folder(&dir)?;
            let moved = suspended_quietly(terminal, || storage.archive_project(&project, year))??;
            Ok(Outcome::Message(lformat!(
                "archived {} to {}",
                project.short_desc(),
                moved.last().map(|p| paths::short(p)).unwrap_or_default()
            )))
        },

        Op::Unarchive { dir } => {
            let storage = open_storage()?;
            let target = storage.unarchive_project_dir(&dir)?;
            if let Some(repo) = storage.repository() {
                let paths = [dir.clone(), target.clone()];
                suspended_quietly(terminal, || repo.add(&paths))?;
            }
            Ok(Outcome::Message(lformat!("moved back to {}", paths::short(&target))))
        },

        Op::Delete { dir } => {
            let storage = open_storage()?;
            let project = Project::open_folder(&dir)?;
            let name = project.short_desc();
            suspended_quietly(terminal, || storage.delete_project_if(&project, || true))??;
            Ok(Outcome::Message(lformat!("deleted {}", name)))
        },

        Op::NewProject {
            name,
            template,
            fill,
            edit,
        } => {
            let storage = storage::setup::<Project>()?;
            let fill: HashMap<&str, String> = fill.into_iter().collect();
            let project = storage.create_project(&name, &template, &fill)?;
            let file = project.file();
            if edit {
                let editor = editor();
                suspended(terminal, false, || util::pass_to_command(Some(&editor), &[&file]))??;
            }
            Ok(Outcome::Created {
                message: lformat!("created {}", paths::short(&file)),
                file,
            })
        },

        Op::ExportCsv { year } => {
            let csv = actions::csv(year)?;
            let path = export_path(year, false);
            fs::write(&path, csv)?;
            Ok(Outcome::File {
                title: lformat!("CSV written"),
                message: lformat!("written {}", paths::short(&path)),
                path,
            })
        },

        Op::ExportIcs { year } => {
            let calendar = actions::calendar_with_tasks(StorageDir::Year(year), true)?;
            let path = export_path(year, true);
            fs::write(&path, calendar)?;
            Ok(Outcome::File {
                title: lformat!("Calendar written"),
                message: lformat!("written {}", paths::short(&path)),
                path,
            })
        },

        Op::Git(git) => run_git(git, terminal),
    }
}

/// Runs `f` with stdin connected to `/dev/null`, so child processes (pdflatex) and prompts
/// (`util::really`) see end of input instead of blocking on the keyboard.
#[cfg(unix)]
fn without_stdin<R>(f: impl FnOnce() -> R) -> Result<R, Error> {
    use std::os::fd::AsRawFd;

    let null = fs::File::open("/dev/null")?;
    // SAFETY: plain fd juggling on fd 0; the saved copy is restored and closed below.
    let saved = unsafe { libc::dup(0) };
    if saved < 0 || unsafe { libc::dup2(null.as_raw_fd(), 0) } < 0 {
        if saved >= 0 {
            unsafe { libc::close(saved) };
        }
        bail!(lformat!("could not detach the keyboard: {}", io::Error::last_os_error()));
    }
    let result = f();
    unsafe {
        libc::dup2(saved, 0);
        libc::close(saved);
    }
    Ok(result)
}

#[cfg(not(unix))]
fn without_stdin<R>(f: impl FnOnce() -> R) -> Result<R, Error> {
    Ok(f())
}

/// For library calls that may let `git add` write to the terminal: run them, then redraw everything.
fn suspended_quietly<R>(terminal: &mut DefaultTerminal, f: impl FnOnce() -> R) -> Result<R, Error> {
    let result = f();
    // wipe whatever was written and redraw everything, without a cursor position query
    execute!(stdout(), Clear(ClearType::All)).map_err(ScreenLost)?;
    *terminal = Terminal::new(CrosstermBackend::new(stdout())).map_err(ScreenLost)?;
    Ok(result)
}

fn run_git(op: GitOp, terminal: &mut DefaultTerminal) -> Result<Outcome, Error> {
    let storage = open_storage()?;
    let root = storage.root_dir().to_owned();
    let Some(repo) = storage.repository() else {
        bail!(lformat!("the storage is not a git repository"));
    };

    let captured = |title: String, args: &[&str]| -> Result<Outcome, Error> {
        Ok(Outcome::Output {
            title,
            text: git_captured(&root, args)?,
        })
    };

    match op {
        GitOp::Status => captured(lformat!("git status"), &["status"]),
        GitOp::Diff => captured(lformat!("git diff"), &["diff", "HEAD"]),
        GitOp::Log => captured(
            lformat!("git log"),
            &["log", "--graph", "--date=relative", "--pretty=format:%h %ad %an  %s", "-n", "300"],
        ),
        GitOp::AddAll => captured(lformat!("git add --all"), &["add", "--all", "--verbose"]),
        GitOp::Commit => {
            let status = suspended(terminal, true, || repo.commit())?;
            Ok(Outcome::Message(lformat!("git commit finished: {}", status)))
        },
        GitOp::Pull => {
            let status = suspended(terminal, true, || repo.pull())?;
            Ok(Outcome::Message(lformat!("git pull finished: {}", status)))
        },
        GitOp::Push => {
            let status = suspended(terminal, true, || repo.push())?;
            Ok(Outcome::Message(lformat!("git push finished: {}", status)))
        },
    }
}
