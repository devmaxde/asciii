# asciii

[![build](https://img.shields.io/github/actions/workflow/status/ascii-dresden/asciii/ci.yml?branch=main)](https://github.com/ascii-dresden/asciii/actions?query=workflow%3A"Continuous+Integration")
[![license](https://img.shields.io/crates/l/asciii.svg)](https://crates.io/crates/asciii/)
[![crates.io](https://img.shields.io/crates/d/asciii.svg)](https://crates.io/crates/asciii)
[![version](https://img.shields.io/crates/v/asciii.svg)](https://crates.io/crates/asciii/)
[![documentation](https://docs.rs/asciii/badge.svg)](https://docs.rs/asciii/)

The **a**dvanced but **s**imple **c**ommandline **i**nterface for **i**nvoice **i**nvocation.

NoSql, blockchain based, serverless, cross-platform project management tool.

Here I rewrite the original [ascii-invoicer](http://github.com/ascii-dresden/ascii-invoicer) in Rust. Why? Because!

## Introduction

The ascii-invoicer is a command-line tool that manages projects and stores them not in a database but in a folder structure. New projects can be created from templates and are stored in a working directory. Projects can be archived, each year will have its own archive. A project consists of a folder containing a yaml file describing it and a number of attached files, such tex files. Projects can contain products and personal. You can create preliminary offers and invoices from your projects.


## Installation

To use \(experimental\) features use the `--features` flag.
Please check the `Cargo.toml` for an up-to-date list of features.
To get a full cli tool build with the feature `full_tool`.

Minimum required version of rustc: `1.32`

### Archlinux

You can install the package `asciii-git` from the AUR.

### macOS

You can install asciii via homebrew.

```sh
$ brew tap ascii-dresden/formulae
$ brew install asciii
```

### Using cargo

Just plain old `cargo install --git https://github.com/ascii-dresden/asciii` or `cargo install asciii` will do.

### Requirements

You need at least `rustc`, `cargo`, `cmake`, `git` and `zlib1g-dev` to run this.

## Development

**Hint!** After the first build you can removed the content of `build.rs`s `fn main()` during dev for significantly improved compile times :D

### Ubuntu 16.04

We recommend installing [rustup](https://github.com/rust-lang-nursery/rustup.rs) with the following command:

```sh
$ curl https://sh.rustup.rs -sSf | sh
```

This installs `rustc`, `cargo`, `rustup` and other standard tools.

#### Ubuntu Requirements

You need at least `cmake` and `zlib1g-dev` to run this.

## Usage
After installation simply run `asciii` and it will present you with a list of possible subcommands. `asciii help list` will give you a comprehensive explanation of  what `asciii list` does.

You can also run `asciii doc` which will take you to the complete [online user and development documentation](http://ascii-dresden.github.io/asciii/).
Further information may be found in the [README of version 2.5](https://github.com/ascii-dresden/ascii-invoicer/blob/master/README.md)

### Terminal UI (`asciii tui`)

`asciii tui` opens an interactive terminal interface (built with [ratatui](https://ratatui.rs)). It starts on a dashboard for the selected year:

* the **expected total** of the year (incl. VAT, excl. VAT where there is room) and the three parts it is made of, each euro counted once: **received** (paid invoices), **waiting for payment** (invoiced, not paid; overdue ones highlighted) and **not yet invoiced** (with how many of those events are already over)
* wages, kept apart as costs: open and paid
* invoiced revenue per month, the events of the next 30 days, a to-do list (invoices to write, payments to chase, staff to pay, projects to archive) and open wages per employee

**Finding your way.** Everything the TUI can do is listed in one place: `Ctrl+P` (also `F1` or `:`) opens the command palette. Type a few letters to find a command or a project ("Open Sommerball"); commands that do not work right now stay visible, greyed out, with the reason (no project open, invoice number already set, pdflatex not installed, …). The footer always shows the most useful keys of the current screen, `?` shows a scrollable help of all keys, and the top line shows where you are (`Projects › Sommerball › Offer`). `Esc` goes one step back and never quits; The tabs are *Dashboard*, *Projects*, *Template* and *Log* (`1`–`4`, `Tab` cycles, or click). `q` quits only on the tabs; `Ctrl+Q`/`Ctrl+C` quit from anywhere and ask first when the offer or the template has unsaved changes. `Ctrl+S` saves in the offer editor, the Template tab and in forms. Messages appear in a line above the footer; errors stay until the next key, details are in the *Log* tab.

**Mouse.** Tabs, the `◀ year ▶` arrows, list rows (click selects, a second click or a double click opens), to-dos, the Actions panel, table headers (sorting), the filter chips, dialog buttons, form fields and the footer hints are clickable, and the wheel scrolls what is under the pointer. To select text with the mouse, hold Shift while dragging (Option in macOS Terminal), or switch the mouse off in the palette ("Mouse: on/off").

The *Projects* tab lists the projects of the year, of all years, or the folders that cannot be read (with the reason, `Enter` shows all of it), with search (`/`) and sorting. The project page shows the event, client, offer/invoice bill and staff; next to it (or with `Enter` on narrow terminals) the **Actions** panel lists what can be done with the project, the next step first. From there you fill in the client, dates and invoice number in one form (`p`), assign the next invoice number (`Shift+I`), create documents (`m`), edit the file (`e`), and archive, unarchive or delete projects. `g` opens a git menu, `x` and `c` export the year as CSV or `.ics` (after asking). Without a project folder the TUI starts on a page that shows where it looked, what is missing and how to fix it, and can create an empty project folder.

Fields like the invoice number, dates and client names are set by their place in the YAML structure, so the template's `##PLACEHOLDER##` marks do not need to be removed by hand; only changed fields are written. Dates can be typed as `24.12.2026`, `24.12.26` or `24.12.`. Placeholders left inside text values are shown as a warning on the project page.

The fixed header of the project page shows the **lifecycle** as one line, `✓ Offer ─ ✓ Accepted ─ ▶ Invoice ─ ○ Paid …` (done, current, still open), with what the current step needs right under it; clicking the current step runs it. The project list shows the next step in its own column. `Shift+A` marks that the client accepted the offer (it writes the optional `accepted: true` under `offer:`; the library and the CLI ignore it, and without it the offer counts as accepted once the event took place or an invoice exists). After the catering, `u` opens the same table as the offer editor for the **actual usage**: per product what was used (`+`/`-`, `=` as offered, `0` nothing, `Enter` any number) and products that were used without being offered (`a`, `n`). It is saved as `sold:`/`returned:` like before, so the invoice is built from it.

`o` on the project page opens the **offer editor**: add products from the project catalog or the template (`a`), create new ones (`n`), change name, unit, price and quantity (`Enter`, `+`/`-`) and remove them (`d`) without touching the YAML file. Only the changed lines are written; leaving with unsaved changes asks whether to save, discard or keep editing. When the catalog of a project changed (in the offer editor or in your editor with `e`), the TUI asks whether the new products and prices should go into the template too; each change can be chosen (`Space`), and `Esc` keeps the template as it is (e.g. for special prices of one customer).

The *Template* tab edits what new projects start with: the **catalog** (product, unit, price) and the **starting offer** (how many of each product a new project offers) of a template in the templates folder. With more than one template, `‹ name ›` at the top chooses (`←`/`→` or click); the configured default (`template` in the configuration) is preselected and marked. The keys are those of the offer editor: `n` new product, `Enter`/`e` change name, unit, price and starting quantity, `+`/`-` the starting quantity (0 takes it out of the starting offer), `d` remove (asks first; a product of the starting offer goes there too), `/` search, `s`/`Ctrl+S` save, `Shift+E` the template file in your editor. Usage numbers left in the starting offer (e.g. `sold: 5`, shown as "starts with usage 5") are cleared with `u`. Existing projects keep their own catalog. Saving works like the offer editor: only the changed lines are written, comments, alignment and `##PLACEHOLDER##` marks stay, renamed products keep their anchor, new ones get a unique anchor from their name, nothing is written if the file changed on disk meanwhile, and the result is read back and checked, including that a new project made from it opens and has the same products. Without a template, the tab explains why and offers to create the default one.

The UI is English, with German translations when `LANG` is German.

The TUI is the optional cargo feature `tui`, off by default, so a normal build does not compile ratatui at all. Build or run it with `cargo run --features tui -- tui` (`cargo install --path . --features tui`, or with nix `nix run .#asciii-tui -- tui` / `callPackage ./package.nix { withTui = true; }`). Without the feature `asciii` behaves exactly like before, just without the `tui` subcommand. Its tests (including render tests at 80×24 and 120×36) run with `cargo test --features tui --bin asciii`, since the binary is `test = false` in `Cargo.toml`.

### ZUGFeRD e-invoices

With `document_export/zugferd: true` in the config, `asciii make --invoice` (and `m` in the TUI) make invoices
as **ZUGFeRD / Factur-X** (profile EN 16931): the invoice data is written as XML (UN/CEFACT CII) next to the
PDF (`R2026-030 Name.xml`) and attached to the PDF as `factur-x.xml`, which turns it into a PDF/A-3b. The CLI
itself does not change. The numbers come from the same bill the PDF is made from.

The seller data is not part of the projects, so it goes into a `zugferd:` section of the `.asciii.yml` in the
storage directory (it travels with the projects in git) or into the normal config; see the commented example in
[`src/default_config.yml`](src/default_config.yml). Needed: seller name, street, postcode, city, VAT ID or tax
number, IBAN, and an exemption reason if items have 0 % VAT (e.g. service hours without tax). The buyer comes from
`client/address` (first line name, the line before `PLZ Ort` the street, an optional country line at the end).
An optional `client/reference` in a project becomes the buyer reference (Leitweg-ID for public clients).

When something is missing, `make` stops before pdflatex runs and says what. The TUI shows it on the project page
(*E-invoice (ZUGFeRD)*) and in the invoice step of the lifecycle. Existing PDFs are not changed; `make --force`
rebuilds them.

asciii checks the required fields, not the full rule set. Check a few invoices with a validator, e.g.
[Mustang](https://www.mustangproject.org) (`java -jar Mustang-CLI.jar --action validate --source file.pdf`) or
veraPDF, and with your accountant's software.

It is the cargo feature `zugferd` (on by default, adds the `lopdf` crate).

### Logging

`asciii` uses Rusts [env_logger](http://doc.rust-lang.org/log/env_logger).
To enable logging you have to set `ASCIII_LOG=debug`.
Besides `debug`, you can also use `trace`, `warn` or `error`.
You can enable logging per-module logging like this: `ASCIII_LOG=storage=debug`.
Modules are all top-level files and folders in `src/`.

### Localization
When you build with the `"localize"` feature then the `lang/default.pot` should be updated automatically during the build. If you have `gettext` installed you can run 

```
msgmerge -U lang/de.po lang/default.pot
```

to update the german local file `lang/de.po`. Now you only need to update any empty field and check the file in as well.

## Features
asciii comes with different sets of feature configurations, most of which are there to speed up development time. By default most useful features are turned on, such as `shell` and `localization`. You can build asciii with a reduced features set by passing these arguments to `cargo build`:

### `--no-default-features --features full_tool` 
* everything, plus `meta`, `version_string` and parallel project loading (`par_open`)

### `--no-default-features --features mini_tool` 
* no `shell`, `git_statuses`, `localization`, `meta` or `serde`, just `cli` and `document_export`

Please check `Cargo.toml` for all features.
