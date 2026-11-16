//! Checks run when the TUI starts: external programs and the state of the git
//! remote.

use std::{
	env,
	path::{Path, PathBuf},
	process::Command,
	sync::mpsc::{self, Receiver},
	thread,
};

use asciii::CONFIG;

/// One line of the startup report.
#[derive(Debug, Clone)]
pub struct Check {
	pub ok: bool,
	/// which program, for the checks of [`programs`]
	pub program: Option<Program>,
	pub text: String,
	/// what to do about it
	pub fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
	Git,
	Pdf,
	Editor,
}

/// Where `command` would be run from: a path with a `/` as is, otherwise the
/// first match in `$PATH`.
fn find_program(command: &str) -> Option<PathBuf> {
	let is_executable = |path: &Path| {
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			path
				.metadata()
				.is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
		}
		#[cfg(not(unix))]
		{
			path.is_file()
		}
	};
	if command.contains('/') {
		let path = PathBuf::from(command);
		return is_executable(&path).then_some(path);
	}
	env::split_paths(&env::var_os("PATH")?)
		.map(|dir| dir.join(command))
		.find(|path| is_executable(path))
}

/// The programs asciii runs: git, the PDF tool from the config and the editor.
pub fn programs(editor: &str) -> Vec<Check> {
	let convert = CONFIG.get_str("document_export/convert_tool");
	let wanted = [
        (
            Program::Git,
            "git",
            lformat!("needed for the git menu and the change markers"),
            lformat!("install git"),
        ),
        (
            Program::Pdf,
            convert,
            lformat!("needed to create PDFs"),
            lformat!("install TeX Live (it brings pdflatex), or set document_export/convert_tool in ~/.asciii.yml"),
        ),
        (
            Program::Editor,
            editor,
            lformat!("needed for \"Edit the project file\""),
            lformat!("set $EDITOR before starting asciii, e.g. export EDITOR=\"code --wait\""),
        ),
    ];
	wanted
		.into_iter()
		.map(|(kind, command, purpose, fix)| {
			let program = command.split_whitespace().next().unwrap_or_default();
			match find_program(program) {
				Some(path) => Check {
					ok: true,
					program: Some(kind),
					text: lformat!("{} found at {}", program, path.display()),
					fix: None,
				},
				None => Check {
					ok: false,
					program: Some(kind),
					text: lformat!("{} not found, {}", program, purpose),
					fix: Some(fix),
				},
			}
		})
		.collect()
}

/// What the checks of [`programs`] mean for the actions.
pub fn tools(checks: &[Check]) -> super::app::Tools {
	let missing = |program: Program| checks.iter().any(|c| c.program == Some(program) && !c.ok);
	let pdf = if !cfg!(feature = "document_export") {
		Err(lformat!("this asciii was built without document export"))
	} else if missing(Program::Pdf) {
		let tool = CONFIG.get_str("document_export/convert_tool");
		Err(lformat!(
			"{} is not installed, PDFs cannot be created here",
			tool
		))
	} else {
		Ok(())
	};
	super::app::Tools {
		pdf,
		editor: !missing(Program::Editor),
		git: !missing(Program::Git),
	}
}

/// What `git fetch` found out about the remote.
#[derive(Debug, Clone)]
pub enum Remote {
	/// commits on the remote that are not here, commits here that are not on the
	/// remote
	Compared { behind: usize, ahead: usize },
	/// the branch does not follow a remote branch
	NoUpstream,
}

fn git(
	root: &Path,
	args: &[&str],
) -> Result<String, String> {
	let mut command = Command::new("git");
	command
		.arg("-C")
		.arg(root)
		.args(args)
		// never ask for passwords, the TUI owns the terminal; a missing key just makes the fetch fail
		.env("GIT_TERMINAL_PROMPT", "0");
	if env::var_os("GIT_SSH_COMMAND").is_none() {
		command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
	}
	let output = command.output().map_err(|e| e.to_string())?;
	if output.status.success() {
		Ok(String::from_utf8_lossy(&output.stdout).into_owned())
	} else {
		Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
	}
}

/// Runs `git fetch` in a background thread and compares the branch with its
/// upstream.
pub fn fetch_in_background(root: PathBuf) -> Receiver<Result<Remote, String>> {
	let (sender, receiver) = mpsc::channel();
	thread::spawn(move || {
		let result = git(&root, &["fetch", "--quiet"]).and_then(|_| {
			match git(
				&root,
				&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
			) {
				Ok(counts) => {
					let mut numbers = counts
						.split_whitespace()
						.map(|n| n.parse::<usize>().unwrap_or(0));
					let ahead = numbers.next().unwrap_or(0);
					let behind = numbers.next().unwrap_or(0);
					Ok(Remote::Compared { behind, ahead })
				},
				Err(_) => Ok(Remote::NoUpstream),
			}
		});
		let _ = sender.send(result);
	});
	receiver
}
