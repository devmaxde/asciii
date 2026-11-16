//! Hooks for the commandline interface
//!
//! # Note to self
//! Put as little logic in here as possible.
//! That makes it easier to derive a pure library version later.

/// Contains concrete implementation of each subcommand
pub mod app;
pub mod subcommands;

#[cfg(feature = "shell")]
pub mod shell;

pub use self::app::{match_matches, with_cli};

use clap::{parser::ValueSource, ArgMatches};

/// clap 3 style accessors on top of the clap 4 `ArgMatches` API.
///
/// Unknown argument ids yield `false`/`None` instead of panicking, which
/// matches how the subcommand handlers probe for arguments that only some
/// subcommands define.
pub trait ArgMatchesExt {
	/// `true` if the argument was given explicitly (flags, options and
	/// positionals alike).
	fn is_present(
		&self,
		id: &str,
	) -> bool;
	/// The value of a single-valued argument.
	fn value_of(
		&self,
		id: &str,
	) -> Option<&str>;
	/// All values of a multi-valued argument.
	fn values_of(
		&self,
		id: &str,
	) -> Option<Box<dyn Iterator<Item = &str> + '_>>;
}

impl ArgMatchesExt for ArgMatches {
	fn is_present(
		&self,
		id: &str,
	) -> bool {
		matches!(self.try_contains_id(id), Ok(true))
			&& !matches!(
				self.value_source(id),
				None | Some(ValueSource::DefaultValue)
			)
	}

	fn value_of(
		&self,
		id: &str,
	) -> Option<&str> {
		self
			.try_get_one::<String>(id)
			.ok()
			.flatten()
			.map(String::as_str)
	}

	fn values_of(
		&self,
		id: &str,
	) -> Option<Box<dyn Iterator<Item = &str> + '_>> {
		self
			.try_get_many::<String>(id)
			.ok()
			.flatten()
			.map(|values| Box::new(values.map(String::as_str)) as Box<dyn Iterator<Item = &str>>)
	}
}
