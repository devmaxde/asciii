//! Aggregated figures over a set of projects (dashboard numbers, to-dos, upcoming events).
//!
//! Read-only on top of the project spec. Everything takes `today` as a parameter so it can be tested.
//! Money semantics follow the `bill` crate: `net_total()` **includes** VAT, `gross_total()` excludes it.

use std::collections::BTreeMap;

use bill::Currency;
use chrono::prelude::*;
use chrono::Duration;

use crate::project::{spec::*, Project};
use crate::storage::Storable;

/// Days a client has to pay an invoice before we follow up (same rule as `Project::to_tasks`).
pub const PAYMENT_GRACE_DAYS: i64 = 14;

/// Days after which a finished project should be archived (same rule as `Project::to_tasks`).
pub const ARCHIVE_AFTER_DAYS: i64 = 7;

/// Adds two amounts. Unlike `Currency + Currency` this does not panic if only one side carries a symbol
/// (the total of an empty bill has none).
pub fn plus(a: Currency, b: Currency) -> Currency {
    Currency {
        symbol: a.symbol.or(b.symbol),
        value: a.value + b.value,
    }
}

/// A number of projects and the sum of their invoice bills.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Amount {
    pub count: usize,
    /// Sum including VAT (`Bill::net_total`)
    pub incl_vat: Currency,
    /// Sum excluding VAT (`Bill::gross_total`)
    pub excl_vat: Currency,
}

impl Amount {
    /// Adds the project's invoice bill. Returns `false` if the bill could not be computed.
    fn add(&mut self, project: &Project) -> bool {
        self.count += 1;
        match project.bills() {
            Ok((_, invoice)) => {
                self.incl_vat = plus(self.incl_vat, invoice.net_total());
                self.excl_vat = plus(self.excl_vat, invoice.gross_total());
                true
            },
            Err(_) => false,
        }
    }
}

/// Dashboard figures for one year.
#[derive(Debug, Clone, Default)]
pub struct YearStats {
    pub year: i32,
    /// Revenue that has been invoiced (invoice date set).
    pub invoiced: Amount,
    /// Expected revenue that is not invoiced yet (not canceled, no invoice date).
    pub open: Amount,
    /// Invoiced and payed by the client.
    pub received: Amount,
    /// Invoiced but not payed yet.
    pub open_bills: Amount,
    /// Open bills whose invoice is older than `PAYMENT_GRACE_DAYS`.
    pub overdue_bills: usize,
    /// Event is over but there is no invoice yet (subset of `open`).
    pub to_invoice: Amount,
    /// Wages (hours × salary) already payed out.
    pub wages_paid: Currency,
    /// Wages (hours × salary) of past events not payed out yet.
    pub wages_open: Currency,
    /// `wages_open` per employee, sorted by name.
    pub open_wages_by_employee: Vec<(String, Currency)>,
    /// Number of projects (including canceled ones).
    pub events: usize,
    pub events_done: usize,
    pub events_upcoming: usize,
    pub canceled: usize,
    /// Sum of service hours of all non canceled projects.
    pub service_hours: f64,
    /// Invoiced revenue (incl. VAT) per month of the event, January = index 0.
    pub monthly_invoiced: [Currency; 12],
    /// Projects whose bill could not be computed (broken `products` section).
    pub unpriced: usize,
}

/// `true` if the event of the project takes place in `year`.
pub fn in_year(project: &Project, year: i32) -> bool {
    project.year() == Some(year)
}

/// `true` if the project belongs in the list of `year`: the year of its event, and also the year its
/// invoice was written in when that is a different one (an event in December invoiced in January).
/// That way the list of a year shows every invoice number of that year, none goes missing.
///
/// Only for listing; figures are computed with [`in_year`] so nothing is counted twice.
pub fn listed_in_year(project: &Project, year: i32) -> bool {
    in_year(project, year) || invoice_year(project) == Some(year)
}

fn days_between(earlier: Date<Utc>, later: Date<Utc>) -> i64 {
    later.signed_duration_since(earlier).num_days()
}

/// Computes the dashboard figures. Callers pass the projects belonging to `year`.
pub fn year_stats(projects: &[&Project], year: i32, today: Date<Utc>) -> YearStats {
    let mut stats = YearStats {
        year,
        ..Default::default()
    };
    let mut open_wages: BTreeMap<String, Currency> = BTreeMap::new();

    for &project in projects {
        stats.events += 1;
        if project.canceled() {
            stats.canceled += 1;
            continue;
        }

        let event = project.event_date().ok();
        let event_is_past = event.is_some_and(|e| e < today);
        if event_is_past {
            stats.events_done += 1;
        } else {
            stats.events_upcoming += 1;
        }

        let mut priced = true;
        if let Ok(invoice_date) = project.invoice().date() {
            priced &= stats.invoiced.add(project);
            if project.is_payed() {
                stats.received.add(project);
            } else {
                stats.open_bills.add(project);
                if days_between(invoice_date, today) >= PAYMENT_GRACE_DAYS {
                    stats.overdue_bills += 1;
                }
            }
            if let (Some(event), Ok((_, invoice))) = (event, project.bills()) {
                let month = event.month0() as usize;
                stats.monthly_invoiced[month] = plus(stats.monthly_invoiced[month], invoice.net_total());
            }
        } else {
            priced &= stats.open.add(project);
            if event_is_past {
                stats.to_invoice.add(project);
            }
        }
        if !priced {
            stats.unpriced += 1;
        }

        let hours = project.hours();
        stats.service_hours += hours.total_time().unwrap_or(0.0);
        if let Some(wages) = hours.gross_wages() {
            if hours.wages_date().is_ok() {
                stats.wages_paid = plus(stats.wages_paid, wages);
            } else if event_is_past {
                stats.wages_open = plus(stats.wages_open, wages);
                for employee in hours.employees().unwrap_or_default() {
                    let bucket = open_wages.entry(employee.name).or_default();
                    *bucket = plus(*bucket, employee.wage);
                }
            }
        }
    }

    stats.open_wages_by_employee = open_wages.into_iter().collect();
    stats
}

/// Something that needs to be done for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// The event is over, write the invoice.
    IssueInvoice,
    /// The invoice has not been payed for `days` days.
    FollowUp { days: i64 },
    /// The client payed, the employees have not been payed yet.
    PayEmployees,
    /// Everything is done, the project can be archived.
    Archive,
    /// Another invoice of the same year has the same number (they must be unique per year).
    /// Not produced by [`tasks`], which looks at single projects; see [`duplicate_invoice_numbers`].
    DuplicateInvoiceNumber,
}

/// A to-do for the project at `index` in the slice passed to [`tasks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    pub kind: TaskKind,
    pub index: usize,
    pub due: Option<Date<Utc>>,
    pub urgent: bool,
}

/// Derives to-dos with the same rules as `Project::to_tasks`, but as data.
/// Pass the projects of the working directory.
pub fn tasks(projects: &[&Project], today: Date<Utc>) -> Vec<Task> {
    let mut tasks = Vec::new();
    for (index, &project) in projects.iter().enumerate() {
        if project.canceled() {
            continue;
        }
        let Ok(event) = project.event_date() else { continue };
        let invoice = project.invoice().date().ok();
        let payed = project.payed_date().ok();

        let task = match (invoice, payed) {
            (None, None) if today >= event => {
                let due = event + Duration::days(PAYMENT_GRACE_DAYS);
                Some((TaskKind::IssueInvoice, Some(due), today > due))
            },
            (Some(invoice), None) => {
                let days = days_between(invoice, today);
                (days >= PAYMENT_GRACE_DAYS).then_some((
                    TaskKind::FollowUp { days },
                    Some(invoice + Duration::days(PAYMENT_GRACE_DAYS)),
                    days > PAYMENT_GRACE_DAYS,
                ))
            },
            (Some(_), Some(payed)) if !project.hours().employees_payed() => {
                let due = payed + Duration::days(PAYMENT_GRACE_DAYS);
                Some((TaskKind::PayEmployees, Some(due), today > due))
            },
            (Some(_), Some(_)) if Project::is_ready_for_archive(project).is_empty() => {
                let finished = project.hours().wages_date().ok().or(payed);
                finished
                    .filter(|f| days_between(*f, today) > ARCHIVE_AFTER_DAYS)
                    .map(|_| (TaskKind::Archive, None, false))
            },
            _ => None,
        };

        if let Some((kind, due, urgent)) = task {
            tasks.push(Task {
                kind,
                index,
                due,
                urgent,
            });
        }
    }
    tasks.sort_by_key(|t| (!t.urgent, t.due));
    tasks
}

/// Indices of non canceled projects whose event is within the next `days` days (today included), sorted by date.
pub fn upcoming(projects: &[&Project], today: Date<Utc>, days: i64) -> Vec<usize> {
    let until = today + Duration::days(days);
    let mut found: Vec<(Date<Utc>, usize)> = projects
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.canceled())
        .filter_map(|(i, p)| p.event_date().ok().map(|d| (d, i)))
        .filter(|(d, _)| *d >= today && *d <= until)
        .collect();
    found.sort();
    found.into_iter().map(|(_, i)| i).collect()
}

/// The year an invoice number belongs to: the year of the invoice date.
///
/// German law (§14 UStG) asks for unique, sequential invoice numbers; the sequence may restart every
/// year, and the year is the one the invoice is written in, not the year of the event.
/// `None` if the project has no invoice date yet.
pub fn invoice_year(project: &Project) -> Option<i32> {
    project.invoice().date().ok().map(|d| d.year())
}

/// Next free invoice number for invoices written in `year`: 1 + the highest number used in that year.
///
/// Pass **all** projects, working and archived. Numbers without an invoice date are counted for every
/// year, which can only skip a number, never hand one out twice.
pub fn next_invoice_number(projects: &[&Project], year: i32) -> u32 {
    1 + projects
        .iter()
        .filter(|p| invoice_year(p).is_none_or(|y| y == year))
        .filter_map(|p| p.field("invoice/number"))
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0)
}

/// Whether `number` is already used by an invoice of `year`; returns the index of that project.
/// `skip` is left out (the project the number is meant for).
pub fn invoice_number_user(projects: &[&Project], year: i32, number: u32, skip: Option<&Project>) -> Option<usize> {
    projects.iter().position(|p| {
        !skip.is_some_and(|s| std::ptr::eq(s, *p))
            && invoice_year(p).is_none_or(|y| y == year)
            && p.invoice().number().ok() == Some(i64::from(number))
    })
}

/// Groups of projects (indices into `projects`) that share an invoice number in the same year.
pub fn duplicate_invoice_numbers(projects: &[&Project]) -> Vec<Vec<usize>> {
    let mut by_number: BTreeMap<(i32, i64), Vec<usize>> = BTreeMap::new();
    for (index, project) in projects.iter().enumerate() {
        if let (Some(year), Ok(number)) = (invoice_year(project), project.invoice().number()) {
            by_number.entry((year, number)).or_default().push(index);
        }
    }
    by_number.into_values().filter(|group| group.len() > 1).collect()
}

/// Which number two projects share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberKind {
    /// offer date and appendix, e.g. `A20261107-1`
    Offer,
    /// invoice year and number, e.g. `R2026-042`
    Invoice,
}

/// A number of one project that another project uses too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberConflict {
    pub kind: NumberKind,
    pub number: String,
    /// index into the `others` slice passed to [`number_conflicts`]
    pub other: usize,
}

/// Offer and invoice numbers of `project` that are used by one of `others` as well.
///
/// Offer numbers are compared as generated from the offer date and appendix, invoice numbers
/// together with the year of the invoice date, so `42` in 2025 and `42` in 2026 do not clash.
/// `project` itself may be part of `others`, it is skipped by identity.
pub fn number_conflicts(project: &Project, others: &[&Project]) -> Vec<NumberConflict> {
    let offer = project.offer().number().ok();
    let invoice = project.invoice().number_long_str();
    let mut conflicts = Vec::new();

    for (index, other) in others.iter().enumerate() {
        if std::ptr::eq(*other, project) {
            continue;
        }
        if let Some(number) = &offer {
            if other.offer().number().ok().as_ref() == Some(number) {
                conflicts.push(NumberConflict {
                    kind: NumberKind::Offer,
                    number: number.clone(),
                    other: index,
                });
            }
        }
        if let Some(number) = &invoice {
            if other.invoice().number_long_str().as_ref() == Some(number) {
                conflicts.push(NumberConflict {
                    kind: NumberKind::Invoice,
                    number: number.clone(),
                    other: index,
                });
            }
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// project with only the numbering fields set
    fn numbered(offer_date: &str, appendix: u32, invoice: Option<(u32, &str)>) -> Project {
        let invoice = match invoice {
            Some((number, date)) => format!("invoice:\n  number: {}\n  date: {}\n", number, date),
            None => String::from("invoice:\n  number:\n"),
        };
        let content = format!(
            "event:\n  name: Test\n  dates:\n  - begin: 01.06.2026\n\
             offer:\n  date: {}\n  appendix: {}\n{}manager: Somebody\nmeta:\n  format: 3.0.0\n",
            offer_date, appendix, invoice
        );
        Project::from_file_content(&content).unwrap()
    }

    #[test]
    fn duplicate_offer_and_invoice_numbers_are_found() {
        let me = numbered("07.11.2026", 1, Some((42, "10.11.2026")));
        let same_offer = numbered("07.11.2026", 1, None);
        let other_appendix = numbered("07.11.2026", 2, None);
        let same_invoice = numbered("01.10.2026", 1, Some((42, "02.10.2026")));
        let invoice_other_year = numbered("01.10.2025", 1, Some((42, "02.10.2025")));
        let all = [&me, &same_offer, &other_appendix, &same_invoice, &invoice_other_year];

        let conflicts = number_conflicts(&me, &all);

        assert_eq!(
            conflicts,
            vec![
                NumberConflict {
                    kind: NumberKind::Offer,
                    number: String::from("A20261107-1"),
                    other: 1
                },
                NumberConflict {
                    kind: NumberKind::Invoice,
                    number: String::from("R2026-042"),
                    other: 3
                },
            ]
        );
        assert!(number_conflicts(&other_appendix, &all).is_empty());
    }

    fn date(d: u32, m: u32, y: i32) -> Date<Utc> {
        Utc.ymd(y, m, d)
    }

    /// minimal project: 4 × 2.50 (19% VAT) + 2h service at 10.00 (no VAT)
    fn project(event: &str, invoice: Option<(u32, &str)>, payed: Option<&str>, wages: Option<&str>, canceled: bool) -> Project {
        let invoice_block = match invoice {
            Some((number, date)) => format!("invoice:\n  number: {}\n  date: {}\n", number, date),
            None => String::from("invoice:\n  number:\n"),
        };
        let invoice_block = match payed {
            Some(payed) => format!("{}  payed_date: {}\n", invoice_block, payed),
            None => invoice_block,
        };
        let wages = wages.map(|w| format!("  wages_date: {}\n", w)).unwrap_or_default();
        let content = format!(
            "event:\n  name: Test\n  dates:\n  - begin: {event}\n{invoice_block}\
             products:\n  Kaffee:\n    amount: 4\n    price: 2.5\n\
             hours:\n  salary: 10.0\n  caterers:\n    Anna: 2\n{wages}\
             manager: Somebody\ncanceled: {canceled}\ntax: 0.19\nmeta:\n  format: 3.0.0\n",
            event = event,
            invoice_block = invoice_block,
            wages = wages,
            canceled = if canceled { "yes" } else { "no" },
        );
        Project::from_file_content(&content).unwrap()
    }

    #[test]
    fn revenue_is_split_into_invoiced_and_open() {
        let today = date(1, 7, 2026);
        let invoiced_payed = project("01.03.2026", Some((1, "02.03.2026")), Some("10.03.2026"), Some("12.03.2026"), false);
        let invoiced_open = project("01.04.2026", Some((2, "02.04.2026")), None, None, false);
        let not_invoiced_past = project("01.06.2026", None, None, None, false);
        let upcoming = project("01.09.2026", None, None, None, false);
        let canceled = project("01.05.2026", None, None, None, true);
        let all = [&invoiced_payed, &invoiced_open, &not_invoiced_past, &upcoming, &canceled];

        let stats = year_stats(&all, 2026, today);

        assert_eq!(stats.events, 5);
        assert_eq!(stats.canceled, 1);
        assert_eq!(stats.events_done, 3);
        assert_eq!(stats.events_upcoming, 1);
        assert_eq!(stats.invoiced.count, 2);
        assert_eq!(stats.open.count, 2);
        assert_eq!(stats.to_invoice.count, 1);
        assert_eq!(stats.received.count, 1);
        assert_eq!(stats.open_bills.count, 1);
        assert_eq!(stats.overdue_bills, 1);
        assert_eq!(stats.unpriced, 0);

        // 10.00 products + 20.00 service, excluding VAT
        assert_eq!(stats.invoiced.excl_vat.value(), 2 * 3000);
        assert!(stats.invoiced.incl_vat.value() > stats.invoiced.excl_vat.value());
        assert_eq!(stats.monthly_invoiced[2], stats.monthly_invoiced[3]);
        assert_eq!(stats.monthly_invoiced[5].value(), 0);

        // wages: 2h × 10.00 per project
        assert_eq!(stats.wages_paid.value(), 2000);
        assert_eq!(stats.wages_open.value(), 2 * 2000);
        assert_eq!(stats.open_wages_by_employee.len(), 1);
        assert_eq!(stats.open_wages_by_employee[0].0, "Anna");
        assert_eq!(stats.open_wages_by_employee[0].1.value(), 2 * 2000);
    }

    #[test]
    fn invoice_in_the_next_year_is_listed_in_both_years() {
        let december = project("15.12.2025", Some((1, "05.01.2026")), None, None, false);
        let same_year = project("01.03.2026", Some((2, "02.03.2026")), None, None, false);
        let not_invoiced = project("20.12.2025", None, None, None, false);

        assert!(listed_in_year(&december, 2025));
        assert!(listed_in_year(&december, 2026));
        assert!(!listed_in_year(&december, 2027));
        // the figures still count it once, in the year of the event
        assert!(in_year(&december, 2025));
        assert!(!in_year(&december, 2026));

        assert!(listed_in_year(&same_year, 2026));
        assert!(!listed_in_year(&same_year, 2025));
        assert!(listed_in_year(&not_invoiced, 2025));
        assert!(!listed_in_year(&not_invoiced, 2026));
    }

    #[test]
    fn tasks_follow_the_project_lifecycle() {
        let today = date(1, 7, 2026);
        let needs_invoice = project("01.06.2026", None, None, None, false);
        let late_payment = project("01.04.2026", Some((2, "02.04.2026")), None, None, false);
        let fresh_invoice = project("25.06.2026", Some((3, "28.06.2026")), None, None, false);
        let pay_staff = project("01.03.2026", Some((1, "02.03.2026")), Some("10.03.2026"), None, false);
        let archive = project("01.02.2026", Some((4, "02.02.2026")), Some("10.02.2026"), Some("12.02.2026"), false);
        let future = project("01.09.2026", None, None, None, false);
        let all = [&needs_invoice, &late_payment, &fresh_invoice, &pay_staff, &archive, &future];

        let tasks = tasks(&all, today);
        let kinds: Vec<(usize, TaskKind)> = tasks.iter().map(|t| (t.index, t.kind)).collect();

        assert!(kinds.contains(&(0, TaskKind::IssueInvoice)));
        assert!(kinds.contains(&(1, TaskKind::FollowUp { days: 90 })));
        assert!(kinds.contains(&(3, TaskKind::PayEmployees)));
        assert!(kinds.contains(&(4, TaskKind::Archive)));
        assert!(!kinds.iter().any(|(i, _)| *i == 2 || *i == 5));
        assert!(tasks[0].urgent, "urgent tasks come first");
    }

    #[test]
    fn upcoming_and_next_invoice_number() {
        let today = date(1, 7, 2026);
        let soon = project("10.07.2026", None, None, None, false);
        let later = project("05.07.2026", None, None, None, false);
        let far = project("01.12.2026", None, None, None, false);
        let invoiced = project("01.04.2026", Some((7, "02.04.2026")), None, None, false);
        let all = [&soon, &later, &far, &invoiced];

        assert_eq!(upcoming(&all, today, 30), vec![1, 0]);
        assert_eq!(next_invoice_number(&all, 2026), 8);
        assert_eq!(next_invoice_number(&[], 2026), 1);
    }

    #[test]
    fn invoice_numbers_count_per_invoice_year() {
        // event in December 2025, invoice written in January 2026: belongs to the 2026 sequence
        let late = project("15.12.2025", Some((3, "10.01.2026")), None, None, false);
        let current = project("01.03.2026", Some((2, "05.03.2026")), None, None, false);
        let last_year = project("01.06.2025", Some((40, "05.06.2025")), None, None, false);
        let all = [&late, &current, &last_year];

        assert_eq!(next_invoice_number(&all, 2026), 4);
        assert_eq!(next_invoice_number(&all, 2025), 41);
        // a new year starts again at 1
        assert_eq!(next_invoice_number(&all, 2027), 1);

        assert_eq!(invoice_number_user(&all, 2026, 3, None), Some(0));
        assert_eq!(invoice_number_user(&all, 2026, 3, Some(&late)), None);
        assert_eq!(invoice_number_user(&all, 2026, 40, None), None);
        assert_eq!(invoice_number_user(&all, 2025, 40, None), Some(2));
    }

    #[test]
    fn duplicate_invoice_numbers_need_the_same_year() {
        let a = project("01.03.2026", Some((5, "05.03.2026")), None, None, false);
        let b = project("01.04.2026", Some((5, "05.04.2026")), None, None, false);
        let other_year = project("01.04.2025", Some((5, "05.04.2025")), None, None, false);
        let unique = project("01.05.2026", Some((6, "05.05.2026")), None, None, false);
        let all = [&a, &b, &other_year, &unique];

        assert_eq!(duplicate_invoice_numbers(&all), vec![vec![0, 1]]);
    }
}
