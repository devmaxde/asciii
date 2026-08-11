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

use clap::{parser::ValuesRef, ArgMatches};

/// The iterator [`ArgMatchesExt::values_of`] hands back.
pub type Values<'a> = std::iter::Map<ValuesRef<'a, String>, fn(&String) -> &str>;

/// Restores the `ArgMatches` accessors that clap 4 removed.
///
/// The `matches_to_*` helpers in [`subcommands`] are handed the `ArgMatches` of
/// whichever subcommand was invoked, so they routinely ask for arguments that the
/// current subcommand does not define at all. clap 4's inherent replacements
/// (`get_flag`, `get_one`, `contains_id`) panic on an unknown id, so everything
/// here goes through the non-panicking `try_*` family instead.
pub trait ArgMatchesExt {
    fn is_present(&self, name: &str) -> bool;
    fn value_of(&self, name: &str) -> Option<&str>;
    fn values_of(&self, name: &str) -> Option<Values<'_>>;
}

impl ArgMatchesExt for ArgMatches {
    fn is_present(&self, name: &str) -> bool {
        // `ArgAction::SetTrue` flags are always "contained" because clap defaults
        // them to `false`, so a flag has to be asked for its actual boolean.
        if let Ok(Some(&flag)) = self.try_get_one::<bool>(name) {
            return flag;
        }
        // Everything else counts as present when it occurred, even without a value
        // (`--archive` with its optional year left off, for instance).
        self.try_contains_id(name).unwrap_or(false)
    }

    fn value_of(&self, name: &str) -> Option<&str> {
        self.try_get_one::<String>(name).ok().flatten().map(String::as_str)
    }

    fn values_of(&self, name: &str) -> Option<Values<'_>> {
        self.try_get_many::<String>(name)
            .ok()
            .flatten()
            .map(|values| values.map(String::as_str as fn(&String) -> &str))
    }
}
