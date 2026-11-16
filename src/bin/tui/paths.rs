//! Paths for people: relative to the project storage or with `~`, and shortened
//! in the middle.

use std::{
	path::{Component, Path, PathBuf},
	sync::OnceLock,
};

/// `a/./b` → `a/b`, `a/x/../b` → `a/b`, without touching the file system.
pub fn clean(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {},
			Component::ParentDir => {
				if !out.pop() {
					out.push("..");
				}
			},
			other => out.push(other.as_os_str()),
		}
	}
	out
}

fn storage_root() -> &'static Option<PathBuf> {
	static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
	// `get_storage_path` panics without a home or current folder
	ROOT.get_or_init(|| {
		(dirs::home_dir().is_some() && std::env::current_dir().is_ok())
			.then(|| clean(&asciii::storage::get_storage_path()))
	})
}

/// Where the configuration says the projects are (absolute, cleaned).
pub fn storage() -> Option<&'static Path> {
	storage_root().as_deref()
}

/// `path` relative to `root`, else with `~` for the home folder, else as it is;
/// never with `/./`.
pub fn relative_to(
	path: &Path,
	root: Option<&Path>,
	home: Option<&Path>,
) -> String {
	let path = clean(path);
	if let Some(rest) = root.and_then(|root| path.strip_prefix(root).ok()) {
		if !rest.as_os_str().is_empty() {
			return rest.display().to_string();
		}
	}
	if let Some(rest) = home.and_then(|home| path.strip_prefix(home).ok()) {
		return Path::new("~").join(rest).display().to_string();
	}
	path.display().to_string()
}

/// A path as shown on screen: relative to the project storage, or with `~`.
pub fn short(path: &Path) -> String {
	let home = dirs::home_dir().map(|h| clean(&h));
	relative_to(path, storage_root().as_deref(), home.as_deref())
}

/// A path as shown on screen, outside the storage: with `~` for the home
/// folder.
pub fn tilde(path: &Path) -> String {
	let home = dirs::home_dir().map(|h| clean(&h));
	relative_to(path, None, home.as_deref())
}

/// Shortens `text` to `max` characters by cutting out the middle:
/// `working/…/Konferenz.yml`.
pub fn middle(
	text: &str,
	max: usize,
) -> String {
	let count = text.chars().count();
	if count <= max {
		return text.to_owned();
	}
	if max <= 1 {
		return String::from("…");
	}
	let keep = max - 1;
	let head = keep / 2;
	let tail = keep - head;
	let start: String = text.chars().take(head).collect();
	let end: String = text.chars().skip(count - tail).collect();
	format!("{}…{}", start, end)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn relative_paths_without_dot_artefacts() {
		let root = Path::new("/home/me/caterings");
		let home = Path::new("/home/me");
		assert_eq!(
			relative_to(
				Path::new("/home/me/./caterings/working/Konferenz/Konferenz.yml"),
				Some(root),
				Some(home)
			),
			"working/Konferenz/Konferenz.yml"
		);
		assert_eq!(
			relative_to(Path::new("/home/me/out/R1.pdf"), Some(root), Some(home)),
			"~/out/R1.pdf"
		);
		assert_eq!(
			relative_to(Path::new("/srv/x/./y"), Some(root), Some(home)),
			"/srv/x/y"
		);
	}

	#[test]
	fn shortens_in_the_middle() {
		assert_eq!(
			middle("working/Konferenz/Konferenz.yml", 40),
			"working/Konferenz/Konferenz.yml"
		);
		let short = middle("working/Herbstkolloquium/Herbstkolloquium.yml", 20);
		assert_eq!(short.chars().count(), 20);
		assert!(short.starts_with("working/") && short.ends_with(".yml") && short.contains('…'));
	}
}
