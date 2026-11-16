use std::str::FromStr;

use super::subcommands;
use anyhow::{format_err, Error};
use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};

use super::ArgMatchesExt;
use clap_complete::Shell;

#[allow(clippy::cognitive_complexity)]
#[rustfmt::skip]
pub fn with_cli<F> (app_handler:F) where F: Fn(Command) {
    let app =
        Command::new("asciii")
            .author(clap::crate_authors!())
            .version(*asciii::VERSION)
            .about(lformat!("The ascii invoicer III"))
            .subcommand_required(true)
            .arg_required_else_help(true)
            .after_help(asciii::DOCUMENTATION_URL)

            .arg(Arg::new("debug")
                 .help(lformat!("Print errors with full backtrace"))
                 .long("debug")
                 .short('d')
                             .action(ArgAction::SetTrue))

            .subcommand(Command::new("bootstrap")
                        .aliases(["boot", "clone"])
                        .about(lformat!("set's up a new instance"))
                        .long_about(lformat!("set's up a new instance. Clones the repository and initializes the global config file."))
                        .arg(Arg::new("repo")
                             .help(lformat!("Remote repository"))
                             .required(true))
                        .arg(Arg::new("to")
                             .help(lformat!("where to clone to"))
                             .long("to")
                             
                             .required(false))
                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .long("editor")
                             
                             .short('e'))

                       )


            .subcommand(Command::new("new")
                        .about(lformat!("Create a new project"))

                        .arg(Arg::new("name")
                             .help(lformat!("Project name"))
                             .required(true))

                        .arg(Arg::new("date")
                             .help(lformat!("Manually set the date of the project"))
                             .value_parser(validators::is_dmy)
                             .short('d')
                             .long("date")
                             )

                        .arg(Arg::new("description")
                             .help(lformat!("Override the description of the project"))
                             .long("desc")
                             )

                        .arg(Arg::new("template")
                             .help(lformat!("Use a specific template"))
                             .long("template")
                             
                             .short('t'))

                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .long("editor")
                             
                             .short('e'))

                        .arg(Arg::new("manager")
                             .help(lformat!("Override the manager of the project"))
                             .long("manager")
                             .short('m')
                             )

                        .arg(Arg::new("time")
                             .help(lformat!("Manually set the start time of the project"))
                             .long("time")
                             )

                        .arg(Arg::new("time_end")
                             .help(lformat!("Manually set the end time of the project"))
                             .long("time_end")
                             )

                        .arg(Arg::new("length")
                             .help(lformat!("Overrides the duration of the event"))
                             .long("length")
                             )

                        .arg(Arg::new("don't edit")
                             .help(lformat!("Do not edit the file after creation"))
                             .long("dont")
                             .action(ArgAction::SetTrue))

                        )

            .subcommand(Command::new("list")
                        .aliases(["ls", "dir", "la", "l", "lsit"])
                        .about(lformat!("List Projects"))

                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects of a specific year, defaults to the current year"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                             .value_name("year")
                            )

                        .arg(Arg::new("year")
                             .help(lformat!("List projects from that year, archived or not"))
                             .short('y')
                             .long("year")
                             .num_args(0..=1)
                             
                            )

                        .arg(Arg::new("details")
                             .help(lformat!("Add extra fields to print for each project listed"))
                             .short('d')
                             .long("details")
                             
                             .num_args(1..).action(ArgAction::Append)
                            )
                        .arg(Arg::new("filter")
                             .help(lformat!("Filter selection by field content"))
                             .short('f')
                             .long("filter")
                             
                             .num_args(1..).action(ArgAction::Append)
                            )
                        .arg(Arg::new("errors")
                             .help(lformat!("Show Errors for each project"))
                             .long("errors")
                             .short('e')
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("colors")
                             .help(lformat!("Show colors"))
                             .long("colors")
                             .short('c')
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("no-colors")
                             .help(lformat!("Show colors for each project"))
                             .long("no-colors")
                             .short('n')
                             .conflicts_with("colors")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("simple")
                             .help(lformat!("Show non-verbose list"))
                             .long("simple")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("csv")
                             .help(lformat!("Print in csv form"))
                             .long("csv")
                             .conflicts_with("simple")
                             .conflicts_with("verbose")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("verbose")
                             .help(lformat!("Opposite of simple"))
                             .long("verbose")
                             .short('v')
                             .conflicts_with("simple")
                             .conflicts_with("csv")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("sort")
                             .help(lformat!("Sort by :"))
                             .long("sort")
                             .short('s')
                             .value_parser(["date",  "index",  "name",  "manager"])
                             
                            )
                        .arg(Arg::new("all")
                             .help(lformat!("List all projects, ever"))
                             .short('A')
                             .long("all")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("templates")
                             .help(lformat!("List templates"))
                             .long("templates")
                             .short('t')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("years")
                             .help(lformat!("List years in archive"))
                             .long("years")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("paths")
                             .help(lformat!("List paths to each project file"))
                             .long("paths")
                             .short('p')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("broken")
                             .help(lformat!("List broken projects  without project file"))
                             .long("broken")
                             .short('b')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("computed_fields")
                             .help(lformat!("List all computed data fields that can be used with --details"))
                             .long("computed")
                             .short('C')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("nothing")
                             .help(lformat!("Print nothing, expect the fields supplied via --details"))
                             .long("nothing")
                             .short('x')
                             .action(ArgAction::SetTrue))
                        )

            .subcommand(Command::new("open")
                        .about(lformat!("Open storage path"))
                        .group(ArgGroup::new("flags")
                            .args([ "search_term", "templates", "output", "bin" ])
                         )
                        .group(ArgGroup::new("bills")
                            .args([ "offer", "invoice" ])
                         )
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                            )

                        .arg(Arg::new("templates")
                             .help(lformat!("Open path to templates instead"))
                             .long("templates")
                             .short('t')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("output")
                             .help(lformat!("Open path to created documents instead"))
                             .long("output")
                             .short('o')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("bin")
                             .help(lformat!("Open path to current binary instead"))
                             .long("bin")
                             .short('b')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("invoice")
                             .help(lformat!("Open invoice file"))
                             .long("invoice")
                             .short('i')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("offer")
                             .help(lformat!("Open offer file"))
                             .long("offer")
                             .action(ArgAction::SetTrue))
                        )
                        //# TODO: add --invoice and --offer

            .subcommand(Command::new("edit")
                        .aliases(["ed"])
                        .about(lformat!("Edit a specific project"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .required(true)
                             .num_args(1..)
                            )

                        .arg(Arg::new("archive")
                             .help(lformat!("Pick an archived project"))
                             .short('a')
                             .long("archive")
                             
                            )

                        .arg(Arg::new("template")
                             .help(lformat!("Edit a template file, use `list --templates` to learn which."))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .short('e')
                             .long("editor")
                             
                            )
                        )

            .subcommand(Command::new("meta")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(Command::new("edit")
                        .about(lformat!("Edit the meta data store"))

                        .arg(Arg::new("template")
                             .help(lformat!("Edit a template file, use `list --templates` to learn which."))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .short('e')
                             .long("editor")
                             
                            )
                        )
                .subcommand(Command::new("store"))
                .subcommand(Command::new("dump"))
                )

            .subcommand(Command::new("archive")
                        .about(lformat!("Move a Project into the archive"))
                        .arg(Arg::new("search terms")
                             .help(lformat!("Search terms to match the project"))
                             //.required(true)
                             .num_args(1..)
                             .conflicts_with("all")
                            )

                        .arg(Arg::new("force")
                             .help(lformat!("Archives the project, even though it is not completely valid"))
                             .long("force")
                             .short('F')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("all")
                             .help(lformat!("Archives all projects that can be archived"))
                             .long("all")
                             .short('a')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("year")
                             .help(lformat!("Override the year"))
                             .long("year")
                             .short('y')
                             
                            )
                       )

            .subcommand(Command::new("unarchive")
                        .about(lformat!("Move a Project out of the archive"))
                        .arg(Arg::new("year")
                             .help(lformat!("Specify the archive"))
                             .required(true)
                            )
                        .arg(Arg::new("name")
                             .help(lformat!("The name of the project, duh!"))
                             .required(true)
                             .num_args(1..)
                            )
                       )

            .subcommand(Command::new("show")
                        .aliases(["display"])
                        .about(lformat!("Display a specific project"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .required(true)
                             .num_args(1..)
                            )

                        .arg(Arg::new("json")
                             .help(lformat!("Show project as JSON"))
                             .long("json")
                             .short('j')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("ical")
                             .help(lformat!("Show project as iCal"))
                             .long("ical")
                             .short('C')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("yaml")
                             .help(lformat!("Show project as raw yaml"))
                             .long("yaml")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("detail")
                             .help(lformat!("Shows a particular detail"))
                             .long("detail")
                             .short('d')
                             
                             )

                        .arg(Arg::new("archive")
                             .help(lformat!("Pick an archived project"))
                             .long("archive")
                             .num_args(0..=1)
                             .short('a')
                             
                            )

                        .arg(Arg::new("empty fields")
                             .help(lformat!("Shows fields that can be filled automatically"))
                             .long("empty_fields")
                             .short('f')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("errors")
                             .help(lformat!("Shows the errors in this project"))
                             .long("errors")
                             .short('e')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("template")
                             .help(lformat!("Show fields in templates that are filled"))
                             .long("template")
                             .short('t')
                             .action(ArgAction::SetTrue))
                        //#conflicts_with: archive  # this causes a crash

                        .arg(Arg::new("files")
                             .help(lformat!("List files that belong to a project"))
                             .long("files")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("invoice")
                             .help(lformat!("Display values in invoice mode"))
                             .long("invoice")
                             .short('i')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("offer")
                             .help(lformat!("Display values in offer mode"))
                             .long("offer")
                             .short('o')
                             .action(ArgAction::SetTrue))

                        //.arg(Arg::new("hours") //# what used to be --caterers
                        //     .help(lformat!("Display hours"))
                        //     .long("hours")
                        //    )

                        .arg(Arg::new("csv")
                             .help(lformat!("Show as csv"))
                             .long("csv")
                             .short('c')
                             .action(ArgAction::SetTrue))

                        //.arg(Arg::new("markdown")
                        //     .help(lformat!("Show as markdown"))
                        //     .long("markdown")
                        //     .short('m')
                        //    )
                    )

            .subcommand(Command::new("set")
                        .about(lformat!("Set a value in a project file"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .required(true)
                            )

                        .arg(Arg::new("field name")
                             .help(lformat!("Which field to set"))
                             .required(true)
                            )

                        .arg(Arg::new("field value")
                             .help(lformat!("What to put in the field"))
                            )

                        .arg(Arg::new("archive")
                             .help(lformat!("Pick an archived project"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        )

            .subcommand(Command::new("invoice")
                        .about(lformat!("Assign invoice id to project"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .required(true)
                            )

                        .arg(Arg::new("archive")
                             .help(lformat!("Pick an archived project"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        )

            .subcommand(Command::new("path")
                        .about(lformat!("Show storage path"))
                        .group(ArgGroup::new("flags")
                            .args([ "search_term", "templates", "output", "bin" ])
                         )
                        .group(ArgGroup::new("bills")
                            .args([ "offer", "invoice" ])
                         )
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                            )
                        .arg(Arg::new("templates")
                             .help(lformat!("Shows templates path instead"))
                             .long("templates")
                             .short('t')
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("output")
                             .help(lformat!("Shows path to created documents instead"))
                             .long("output")
                             .short('o')
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("bin")
                             .help(lformat!("Open path to current binary instead"))
                             .long("bin")
                             .short('b')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("invoice")
                             .help(lformat!("Open invoice file"))
                             .long("invoice")
                             .short('i')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("offer")
                             .help(lformat!("Open offer file"))
                             .long("offer")
                             .action(ArgAction::SetTrue))
                        )

            .subcommand(Command::new("workspace")
                        .aliases(["ws"])
                         .arg(Arg::new("archive")
                             .help(lformat!("Open an archive instead"))
                             .short('a')
                             .long("archive")
                             
                            )

                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .short('e')
                             .long("editor")
                             
                            )
                        .about(lformat!("Open the working directory in an editor"))
                        )



            .subcommand(Command::new("csv")
                        .about(lformat!("Produces a CSV report for a given year"))
                        .arg(Arg::new("year")
                             .help(lformat!("List projects from that year, archived or not"))
                             //.short('y')
                             //.long("year")
                             .value_parser(|y: &str| y.parse::<i32>().map(|_ok| y.to_owned()).map_err(|e| e.to_string()))
                             
                            )
                        )

            .subcommand(Command::new("calendar")
                        .aliases(["cal","ical","ics","kalender"])
                        .arg(Arg::new("archive")
                             .help(lformat!("List archived projects of a specific year, defaults to the current year"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                             .value_name("year")
                            )

                        .arg(Arg::new("tasks")
                             .help(lformat!("Include open tasks"))
                             .short('t')
                             .long("tasks")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("year")
                             .help(lformat!("List projects from that year, archived or not"))
                             .short('y')
                             .long("year")
                             .num_args(0..=1)
                             
                            )

                        .arg(Arg::new("all")
                             .help(lformat!("List all projects, ever"))
                             .short('A')
                             .long("all")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("dues")
                        .about(lformat!("Experimental: open dues"))

                        .arg(Arg::new("invoices")
                             .help(lformat!("Show unpayed wages"))
                             .long("invoices")
                             .short('i')
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("wages")
                             .help(lformat!("Show unpayed wages"))
                             .long("wages")
                             .short('w')
                             .conflicts_with("invoices")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("make")
                        .about(lformat!("Creates documents from projects"))
                        .aliases(["mk"])

                        .arg(Arg::new("file")
                             .help(lformat!("Manually pass a file path"))
                             .long("file")
                             
                            )

                        .arg(Arg::new("output")
                             .help(lformat!("Manually pass a output folder"))
                             .long("output")
                             
                            )

                        .arg(Arg::new("force")
                             .help(lformat!("Do it against better judgement"))
                             .long("force")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("pdf-only")
                             .help(lformat!("Only create the PDF file"))
                             .long("pdf")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("dry-run")
                             .help(lformat!("Do not create final output file"))
                             .short('d')
                             .long("dry")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("print-only")
                             .help(lformat!("Only prints to stdout"))
                             .long("print")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("open")
                             .help(lformat!("Open the pdf file afterwards."))
                             .long("open")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                             )

                        .arg(Arg::new("offer")
                             .help(lformat!("Produce an offer document"))
                             .long("offer")
                             .conflicts_with("invoice")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("invoice")
                             .help(lformat!("Produce an invoice document"))
                             .long("invoice")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("archive")
                             .help(lformat!("Pick an archived project"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )

                        .arg(Arg::new("template")
                             .help(lformat!("Use a particular template"))
                             .short('t')
                             .long("template")
                             
                             )
                       )

            .subcommand(Command::new("delete")
                        .about(lformat!("Deletes a project"))
                        .aliases(["rm"])

                        .arg(Arg::new("dry-run")
                             .help(lformat!("Do not create final output file"))
                             .short('d')
                             .long("dry")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .required(true)
                             .num_args(1..))

                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        //.arg(Arg::new("template")
                        //     .help(lformat!("A template"))
                        //     .short('t')
                        //     .long("template")
                        //    )
                       )


            .subcommand(Command::new("config")
                        .aliases(["settings"])
                        .about(lformat!("Show and edit your config"))
                        .arg(Arg::new("edit")
                             .help(lformat!("Edit your config"))
                             .short('e')
                             .long("edit")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("editor")
                             .help(lformat!("Override the configured editor"))
                             .long("editor")
                             
                             )

                        .arg(Arg::new("show")
                             .help(lformat!("Show a specific config value"))
                             .short('s')
                             .long("show")
                             
                            )

                        .arg(Arg::new("default")
                             .help(lformat!("Show default config"))
                             .short('d')
                             .long("default")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("set root")
                             .help(lformat!("set the root folder in the config"))
                             .long("set-root")
                             
                            )

                        .arg(Arg::new("location")
                             .help(lformat!("Show the location of the config file"))
                             .short('l')
                             .long("location")
                             .action(ArgAction::SetTrue))

                        .arg(Arg::new("init")
                             .help(lformat!("Create config file."))
                             .short('i')
                             .long("init")
                             .action(ArgAction::SetTrue))

                        )

            .subcommand(Command::new("shell")
                        .aliases(["sh", "repl"])
                        .about(lformat!("(experimental) starts interactive shell"))
                       )

            .subcommand(Command::new("whoami")
                        .about(lformat!("Show your name from config"))
                       )

            //# GIT STUFF
            .subcommand(Command::new("status")
                        .about(lformat!("Show the working tree status"))
                        .aliases(["st"])
                       )

            .subcommand(Command::new("pull")
                        .about(lformat!("Pull and merge new commits from remote"))
                        .aliases(["update"])
                        .arg(Arg::new("rebase")
                             .help(lformat!("git pull with --rebase"))
                             .long("rebase")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("diff")
                        .about(lformat!("git diff"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                            )
                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        .arg(Arg::new("staged")
                             .help(lformat!("Changes between the index and your last commit"))
                             .long("staged")
                             .alias("cached")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("template")
                             .help(lformat!("A template"))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("add")
                        .about(lformat!("Add file contents to the git-index"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                            )
                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        .arg(Arg::new("template")
                             .help(lformat!("A template"))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))
                        .arg(Arg::new("all")
                             .help(lformat!("Add all projects"))
                             .short('A')
                             .long("all")
                             .action(ArgAction::SetTrue))

                       )

            .subcommand(Command::new("commit")
                        .aliases(["cm"])
                        .about(lformat!("Save changes locally"))
                       )

            .subcommand(Command::new("push")
                        .about(lformat!("Upload locally saved changes to the remote"))
                       )

            .subcommand(Command::new("cleanup")
                        .about(lformat!("cleans changes and untracked files in project folder"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                             .required(true)
                            )
                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        .arg(Arg::new("template")
                             .help(lformat!("A template"))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("stash").about(lformat!("equals git stash")))
            .subcommand(Command::new("pop").about(lformat!("equals git pop")))

            .subcommand(Command::new("log")
                        .aliases(["lg", "hist", "history"])
                        .about(lformat!("Show commit logs"))
                        .arg(Arg::new("search_term")
                             .help(lformat!("Search term, possibly event name"))
                             .num_args(1..)
                            )
                        .arg(Arg::new("archive")
                             .help(lformat!("list archived projects"))
                             .short('a')
                             .long("archive")
                             .num_args(0..=1)
                             
                            )
                        .arg(Arg::new("template")
                             .help(lformat!("A template"))
                             .short('t')
                             .long("template")
                             .action(ArgAction::SetTrue))
                       )

            .subcommand(Command::new("remote")
                        .about(lformat!("Show information about the remote"))
                       )

            .subcommand(Command::new("complete")
                        //.aliases(["lg", "hist", "history"])
                        .about(lformat!("Generates completion for bash, zsh, etc"))
                        .arg(Arg::new("shell")
                             .help(lformat!("what shell to generate completion for (bash, zsh, fish,PowerShell)"))
                             .num_args(0..=1)
                             
                             .value_name("shell")
                            )

                       )

            .subcommand(Command::new("version")
                .about(lformat!("Prints version information"))
                .group(ArgGroup::new("flags")
                    .args([ "verbose", "json" ])
                    )
                .arg(Arg::new("verbose")
                        .help(lformat!("show also build information"))
                        .long("verbose")
                        .short('v')
                             .action(ArgAction::SetTrue))
                .arg(Arg::new("json")
                        .help(lformat!("show verbose version as json"))
                        .long("json")
                             .action(ArgAction::SetTrue))
            )

            .subcommand(Command::new("doc")
                .about(lformat!("Opens the online documentation, please read it"))
            )


            .subcommand(Command::new("asciii")
                        .aliases(["asci", "ascii"])
                        .arg(Arg::new("anything").num_args(0..).allow_hyphen_values(true))
                        .hide(true)
            )

            .subcommand(Command::new("nocommand")
                        .aliases([ "shit", "fuck", "git" ])
                        .arg(Arg::new("anything").num_args(0..).allow_hyphen_values(true))
                        .hide(true)
            )

            .subcommand(Command::new("notashell")
                        .aliases(["cp", "cd", "mv", "source", "ssh"])
                        .arg(Arg::new("anything").num_args(0..).allow_hyphen_values(true))
                        .hide(true)
            )

        ;

    #[cfg(feature = "tui")]
    let app = app.subcommand(Command::new("tui")
        .about(lformat!("Interactive terminal interface with dashboard"))
    );

    app_handler(app);
}

/// Starting point for handling commandline matches
#[rustfmt::skip]
pub fn match_matches(matches: &ArgMatches) {
    let res = match matches.subcommand() {
     Some(("bootstrap", sub_m)) => subcommands::bootstrap(sub_m),
     Some(("list",      sub_m)) => subcommands::list(sub_m),
     Some(("csv",       sub_m)) => subcommands::csv(sub_m),
     Some(("new",       sub_m)) => subcommands::new(sub_m),
     Some(("edit",      sub_m)) => subcommands::edit(sub_m),
     Some(("meta",      sub_m)) => subcommands::meta(sub_m),
     Some(("workspace", sub_m)) => subcommands::workspace(sub_m),
     Some(("set",       sub_m)) => subcommands::set(sub_m),
     Some(("invoice",   sub_m)) => subcommands::invoice(sub_m),
     Some(("show",      sub_m)) => subcommands::show(sub_m),
     Some(("calendar",  sub_m)) => subcommands::calendar(sub_m),
     Some(("archive",   sub_m)) => subcommands::archive(sub_m),
     Some(("unarchive", sub_m)) => subcommands::unarchive(sub_m),
     Some(("config",    sub_m)) => subcommands::config(sub_m),
     Some(("whoami",    _          )) => subcommands::config_show("user/name"),
     Some(("nocommand", sub_m)) => subcommands::no_command(sub_m),
     Some(("notashell", sub_m)) => subcommands::no_shell(sub_m),
     Some(("asciii",    sub_m)) => subcommands::double_command(sub_m),

     Some(("path",      sub_m)) => subcommands::show_path(sub_m),
     Some(("open",      sub_m)) => subcommands::open_path(sub_m),

     Some(("make",      sub_m)) => subcommands::make(sub_m),
     Some(("delete",    sub_m)) => subcommands::delete(sub_m),
     Some(("spec",      sub_m)) => subcommands::spec(sub_m),

     Some(("doc",       _          )) => subcommands::doc(),
     Some(("version",   sub_m)) => subcommands::version(sub_m),

     Some(("dues",      sub_m)) => subcommands::dues(sub_m),
     Some(("shell",     sub_m)) => subcommands::shell(sub_m),

     Some(("remote",    _          )) => subcommands::git_remote(),
     Some(("pull",      sub_m)) => subcommands::git_pull(sub_m),
     Some(("diff",      sub_m)) => subcommands::git_diff(sub_m),
     Some(("cleanup",   sub_m)) => subcommands::git_cleanup(sub_m),
     Some(("status",    _          )) => subcommands::git_status(),
     Some(("add",       sub_m)) => subcommands::git_add(sub_m),
     Some(("commit",    _          )) => subcommands::git_commit(),
     Some(("push",      _          )) => subcommands::git_push(),
     Some(("stash",     _          )) => subcommands::git_stash(),
     Some(("pop",       _          )) => subcommands::git_stash_pop(),
     Some(("log",       sub_m)) => subcommands::git_log(sub_m),
     Some(("complete",  sub_m)) => generate_completions(sub_m),
     #[cfg(feature = "tui")]
     Some(("tui",       _          )) => crate::tui::run(),
     _                          => Err(format_err!("unhandled command"))
    };
    if let Err(e) = res {
        if matches.is_present("debug") {
            println!("{:?}", e)
        } else {
            log::error!("{} (Cause: {})", e, e.root_cause());
            log::info!("use --debug to see a backtrace");
        }
    }
}

pub fn generate_completions(matches: &ArgMatches) -> Result<(), Error> {
	if let Some(shell) = matches
		.value_of("shell")
		.and_then(|s| Shell::from_str(s).ok())
	{
		// with_cli(|mut app| app.gen_completions("asciii", shell, ".") );
		with_cli(|mut command| {
			clap_complete::generate(shell, &mut command, "asciii", &mut std::io::stdout())
		});
	} else {
		log::error!(
			"{}",
			lformat!("please specify either bash, zsh, fish or powershell")
		);
	}
	Ok(())
}

pub mod validators {
	use asciii::util::yaml::parse_dmy_date;

	pub fn is_dmy(val: &str) -> Result<String, String> {
		match parse_dmy_date(val) {
			Some(_) => Ok(val.to_owned()),
			None => Err(lformat!("Date Format must be DD.MM.YYYY")),
		}
	}
}
