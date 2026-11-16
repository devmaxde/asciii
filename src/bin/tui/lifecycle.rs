//! Where a project stands: offer → accepted → event → usage → invoice → paid →
//! archived.
//!
//! Everything is derived from the project file as the library reads it. The
//! only addition is the optional `offer/accepted: true`; without it, the offer
//! counts as accepted once the event took place or an invoice exists, so older
//! projects show up right.

use chrono::prelude::*;

use asciii::project::{
	spec::{HasEmployees, HasEvents, Invoicable, IsProject, Offerable, Redeemable},
	Exportable, Project,
};

use super::{data::Location, offer, yaml_edit};

/// The steps, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
	Offer,
	Accepted,
	Event,
	Usage,
	Invoice,
	Paid,
	Archived,
}

impl Step {
	pub const ALL: [Step; 7] = [
		Step::Offer,
		Step::Accepted,
		Step::Event,
		Step::Usage,
		Step::Invoice,
		Step::Paid,
		Step::Archived,
	];

	pub fn label(self) -> String {
		match self {
			Step::Offer => lformat!("Offer"),
			Step::Accepted => lformat!("Accepted"),
			Step::Event => lformat!("Catering"),
			Step::Usage => lformat!("Actual usage"),
			Step::Invoice => lformat!("Invoice"),
			Step::Paid => lformat!("Paid"),
			Step::Archived => lformat!("Archived"),
		}
	}

	/// Shorter names for the step line on narrow pages.
	pub fn short_label(self) -> String {
		match self {
			Step::Offer => lformat!("Offer"),
			Step::Accepted => lformat!("Agreed"),
			Step::Event => lformat!("Catering"),
			Step::Usage => lformat!("Usage"),
			Step::Invoice => lformat!("Invoice"),
			Step::Paid => lformat!("Paid"),
			Step::Archived => lformat!("Archive"),
		}
	}

	/// What is to do when this is the current step, short for the project list.
	pub fn todo(self) -> String {
		match self {
			Step::Offer => lformat!("write offer"),
			Step::Accepted => lformat!("wait for OK"),
			Step::Event => lformat!("catering"),
			Step::Usage => lformat!("note usage"),
			Step::Invoice => lformat!("write invoice"),
			Step::Paid => lformat!("wait for money"),
			Step::Archived => lformat!("archive"),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
	Done,
	/// not noted, but a later step shows it happened
	Implied,
	/// the next thing to do
	Current,
	Open,
	/// not needed (canceled project)
	Skipped,
}

#[derive(Debug, Clone)]
pub struct StepView {
	pub step: Step,
	pub mark: Mark,
	/// what is known about the step
	pub text: String,
	/// what to do and with which key, for open steps
	pub todo: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Lifecycle {
	pub steps: Vec<StepView>,
	/// the first step that is not done, `None` when archived
	pub current: Option<Step>,
	pub canceled: bool,
}

impl Lifecycle {
	pub fn current_view(&self) -> Option<&StepView> {
		self
			.current
			.and_then(|c| self.steps.iter().find(|s| s.step == c))
	}
}

/// What the lifecycle is computed from.
#[derive(Debug, Clone, Default)]
pub struct Facts {
	pub canceled: bool,
	pub offer_number: String,
	pub offer_date: Option<Date<Utc>>,
	pub offer_pdf: bool,
	pub offer_missing: Vec<String>,
	pub accepted: bool,
	pub event_begin: Option<Date<Utc>>,
	pub event_end: Option<Date<Utc>>,
	/// products with noted usage, all products
	pub counted: (usize, usize),
	pub invoice_number: Option<String>,
	pub invoice_date: Option<Date<Utc>>,
	pub invoice_pdf: bool,
	pub invoice_missing: Vec<String>,
	/// ZUGFeRD switched on: what the e-invoice still needs (empty: ready)
	pub e_invoice: Option<Vec<String>>,
	pub paid: Option<Date<Utc>>,
	pub staff: bool,
	pub wages_paid: Option<Date<Utc>>,
	pub archive_missing: Vec<String>,
	pub archived: Option<i32>,
	pub today: Option<Date<Utc>>,
}

/// Reads the facts from a project; `content` is its file.
pub fn facts(
	project: &Project,
	content: &str,
	location: Location,
	today: Date<Utc>,
) -> Facts {
	let events = project.events().unwrap_or_default();
	let event_begin = events
		.iter()
		.map(|e| e.begin)
		.min()
		.or_else(|| project.event_date().ok());
	let event_end = events
		.iter()
		.map(|e| e.end.unwrap_or(e.begin))
		.max()
		.or(event_begin);
	let counted = offer::read(content)
		.map(|o| {
			(
				o.lines.iter().filter(|l| l.counted()).count(),
				o.lines.len(),
			)
		})
		.unwrap_or((0, 0));
	let hours = project.hours();
	let staff = hours
		.employees()
		.map(|e| e.iter().any(|e| e.time > 0.0))
		.unwrap_or(false);
	// the invoice step only needs what the invoice itself is missing, the offer
	// fields are the offer's
	let offer_missing = project.is_missing_for_offer();
	let invoice_missing: Vec<String> = project
		.is_missing_for_invoice()
		.into_iter()
		.filter(|m| !offer_missing.contains(m))
		.collect();
	Facts {
		canceled: project.canceled(),
		offer_number: project.offer().number().unwrap_or_default(),
		offer_date: project.offer().date().ok(),
		offer_pdf: project.offer_file_exists(),
		offer_missing,
		accepted: yaml_edit::value_at(content, "offer/accepted")
			.is_some_and(|v| matches!(v.to_lowercase().as_str(), "true" | "yes" | "ja" | "y")),
		event_begin,
		event_end,
		counted,
		invoice_number: project.invoice().number_long_str(),
		invoice_date: project.invoice().date().ok(),
		invoice_pdf: project.invoice_file_exists(),
		invoice_missing,
		e_invoice: match location {
			Location::Working => e_invoice(project),
			Location::Archive(_) => None,
		},
		paid: project.payed_date().ok(),
		staff,
		wages_paid: hours.wages_date().ok(),
		archive_missing: Project::is_ready_for_archive(project),
		archived: match location {
			Location::Archive(year) => Some(year),
			Location::Working => None,
		},
		today: Some(today),
	}
}

/// `None` if ZUGFeRD is off, else what the e-invoice still needs.
pub fn e_invoice(project: &Project) -> Option<Vec<String>> {
	#[cfg(feature = "zugferd")]
	{
		asciii::zugferd::enabled().then(|| asciii::zugferd::missing(project))
	}
	#[cfg(not(feature = "zugferd"))]
	{
		let _ = project;
		None
	}
}

fn dmy(date: Option<Date<Utc>>) -> String {
	date
		.map(|d| d.format("%d.%m.%Y").to_string())
		.unwrap_or_default()
}

fn missing(list: &[String]) -> String {
	lformat!("missing: {}", yaml_edit::labels(list))
}

pub fn lifecycle(facts: &Facts) -> Lifecycle {
	let today = facts.today.unwrap_or_else(Utc::today);
	let event_done = facts.event_end.is_some_and(|end| end < today);
	let usage_done = facts.counted.1 > 0 && facts.counted.0 == facts.counted.1;
	// number and date make the invoice; the PDF may live on another machine (output
	// folder)
	let invoice_done = facts.invoice_number.is_some() && facts.invoice_date.is_some();
	let paid_done = facts.paid.is_some() && (!facts.staff || facts.wages_paid.is_some());

	let done = [
		facts.offer_pdf,
		facts.accepted,
		event_done,
		usage_done,
		invoice_done,
		paid_done,
		facts.archived.is_some(),
	];
	// a step counts as happened if a later one did; not for the event (an invoice
	// may come first) and not for the payment (only canceled projects are archived
	// unpaid)
	let can_be_implied = [true, true, false, true, true, false, false];
	let later_done = |i: usize| done[i + 1..].iter().any(|d| *d);

	let mut current = None;
	let steps = Step::ALL
		.iter()
		.enumerate()
		.map(|(i, &step)| {
			let mark = if done[i] {
				Mark::Done
			} else if can_be_implied[i] && later_done(i) {
				Mark::Implied
			} else if facts.canceled && step != Step::Offer && step != Step::Archived {
				Mark::Skipped
			} else if current.is_none() {
				current = Some(step);
				Mark::Current
			} else {
				Mark::Open
			};
			let (text, todo) = describe(step, mark, facts, today);
			StepView {
				step,
				mark,
				text,
				todo,
			}
		})
		.collect();

	Lifecycle {
		steps,
		current,
		canceled: facts.canceled,
	}
}

fn describe(
	step: Step,
	mark: Mark,
	f: &Facts,
	today: Date<Utc>,
) -> (String, Option<String>) {
	let open = matches!(mark, Mark::Current | Mark::Open);
	match step {
		Step::Offer => {
			let mut text = [f.offer_number.clone(), dmy(f.offer_date)]
				.into_iter()
				.filter(|s| !s.is_empty())
				.collect::<Vec<_>>()
				.join(" · ");
			if f.offer_pdf {
				text.push_str(&format!(" · {}", lformat!("PDF created")));
			}
			let todo = open.then(|| {
				if f.offer_missing.is_empty() {
					lformat!("create the offer PDF (m); products: edit the offer (o)")
				} else {
					lformat!(
						"{}; fill in with Project details (p)",
						missing(&f.offer_missing)
					)
				}
			});
			(text, todo)
		},
		Step::Accepted => match mark {
			Mark::Done => (
				lformat!("accepted by the client (Shift+A takes it back)"),
				None,
			),
			Mark::Implied => (lformat!("not marked, but the project went on"), None),
			Mark::Skipped => (lformat!("canceled"), None),
			_ => (
				lformat!("waiting for the client"),
				Some(lformat!(
					"when the client says yes: Client accepted the offer (Shift+A)"
				)),
			),
		},
		Step::Event => {
			let mut text = dmy(f.event_begin);
			if f.event_end.is_some() && f.event_end != f.event_begin {
				text.push_str(&format!(" – {}", dmy(f.event_end)));
			}
			if let Some(begin) = f.event_begin {
				let days = begin.signed_duration_since(today).num_days();
				if days > 1 {
					text.push_str(&format!("  ({})", lformat!("in {} days", days)));
				} else if days == 1 {
					text.push_str(&format!("  ({})", lformat!("tomorrow")));
				} else if days == 0 {
					text.push_str(&format!("  ({})", lformat!("today")));
				}
			} else {
				text = lformat!("no date");
			}
			(text, None)
		},
		Step::Usage => {
			let (counted, all) = f.counted;
			let text = match mark {
				Mark::Done => lformat!("noted for all {} products", all),
				Mark::Implied if counted > 0 => lformat!(
					"noted for {} of {} products, the others count as offered",
					counted,
					all
				),
				Mark::Implied => lformat!("not noted, the invoice uses the offered amounts"),
				Mark::Skipped => lformat!("canceled"),
				_ if all == 0 => lformat!("no products"),
				_ if counted > 0 => lformat!("noted for {} of {} products", counted, all),
				_ => lformat!("not noted yet"),
			};
			let todo = open.then(|| lformat!("note what was actually used, returned or extra (u)"));
			(text, todo)
		},
		Step::Invoice => {
			let mut parts: Vec<String> = [
				f.invoice_number.clone().unwrap_or_default(),
				dmy(f.invoice_date),
			]
			.into_iter()
			.filter(|s| !s.is_empty())
			.collect();
			if f.invoice_pdf {
				parts.push(lformat!("PDF created"));
			} else if f.invoice_number.is_some() && f.invoice_date.is_some() {
				parts.push(lformat!("no PDF here yet (m)"));
			}
			let invoice_set = f.invoice_number.is_some() && f.invoice_date.is_some();
			match &f.e_invoice {
				_ if !invoice_set => {},
				Some(needs) if needs.is_empty() => parts.push(lformat!("e-invoice ready")),
				Some(_) => parts.push(lformat!("e-invoice incomplete")),
				None => {},
			}
			if mark == Mark::Skipped {
				parts = vec![lformat!("canceled")];
			}
			let todo = open.then(|| {
				let mut todo = Vec::new();
				if f.invoice_number.is_none() {
					todo.push(lformat!("assign the invoice number (Shift+I)"));
				}
				if f.invoice_date.is_none() {
					todo.push(lformat!("set the invoice date"));
				}
				if !f.invoice_missing.is_empty() && f.invoice_number.is_some() && f.invoice_date.is_some() {
					todo.push(missing(&f.invoice_missing));
				}
				if let Some(needs) = f.e_invoice.as_ref().filter(|n| !n.is_empty()) {
					todo.push(lformat!("e-invoice needs: {}", needs.join(", ")));
				}
				todo.push(lformat!("create the invoice PDF (m)"));
				todo.join(", ")
			});
			(parts.join(" · "), todo)
		},
		Step::Paid => {
			let client = match f.paid {
				Some(date) => lformat!("client paid {}", dmy(Some(date))),
				None => lformat!("client: open"),
			};
			let staff = match (f.staff, f.wages_paid) {
				(false, _) => lformat!("no staff"),
				(true, Some(date)) => lformat!("wages paid {}", dmy(Some(date))),
				(true, None) => lformat!("wages: open"),
			};
			let text = if mark == Mark::Skipped {
				lformat!("canceled")
			} else {
				format!("{} · {}", client, staff)
			};
			let todo = open.then(|| {
				let mut todo = Vec::new();
				if f.paid.is_none() {
					todo.push(lformat!("client paid: set the date"));
				}
				if f.staff && f.wages_paid.is_none() {
					todo.push(lformat!("wages paid: set the date"));
				}
				todo.join(", ")
			});
			(text, todo)
		},
		Step::Archived => {
			let text = match f.archived {
				Some(year) => lformat!("archive {}", year),
				None => String::new(),
			};
			let todo = open.then(|| {
				if f.archive_missing.is_empty() {
					lformat!("archive the project (a)")
				} else {
					lformat!(
						"{}; archiving anyway is possible (a)",
						missing(&f.archive_missing)
					)
				}
			});
			(text, todo)
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn date(
		d: u32,
		m: u32,
	) -> Option<Date<Utc>> {
		Some(Utc.ymd(2026, m, d))
	}

	fn base() -> Facts {
		Facts {
			offer_number: String::from("A20260901-1"),
			offer_date: date(1, 9),
			event_begin: date(15, 11),
			event_end: date(16, 11),
			counted: (0, 4),
			staff: true,
			today: date(12, 11),
			..Facts::default()
		}
	}

	fn marks(l: &Lifecycle) -> Vec<Mark> {
		l.steps.iter().map(|s| s.mark).collect()
	}

	use Mark::*;

	#[test]
	fn new_project_starts_with_the_offer() {
		let l = lifecycle(&base());
		assert_eq!(l.current, Some(Step::Offer));
		assert_eq!(marks(&l), [Current, Open, Open, Open, Open, Open, Open]);
	}

	#[test]
	fn steps_follow_each_other() {
		let mut f = base();
		f.offer_pdf = true;
		assert_eq!(lifecycle(&f).current, Some(Step::Accepted));
		f.accepted = true;
		// accepted, the event is ahead: waiting for the event
		assert_eq!(lifecycle(&f).current, Some(Step::Event));
		f.today = date(20, 11);
		assert_eq!(lifecycle(&f).current, Some(Step::Usage));
		f.counted = (2, 4);
		assert_eq!(lifecycle(&f).current, Some(Step::Usage));
		f.counted = (4, 4);
		assert_eq!(lifecycle(&f).current, Some(Step::Invoice));
		f.invoice_number = Some(String::from("R2026-040"));
		assert_eq!(lifecycle(&f).current, Some(Step::Invoice));
		f.invoice_date = date(21, 11);
		assert_eq!(lifecycle(&f).current, Some(Step::Paid));
		f.paid = date(1, 12);
		// the staff is not paid yet
		assert_eq!(lifecycle(&f).current, Some(Step::Paid));
		f.wages_paid = date(2, 12);
		assert_eq!(lifecycle(&f).current, Some(Step::Archived));
		f.archived = Some(2026);
		let l = lifecycle(&f);
		assert_eq!(l.current, None);
		assert_eq!(marks(&l), [Done; 7]);
	}

	#[test]
	fn older_projects_imply_acceptance_and_usage() {
		// no `accepted`, no usage noted, but invoiced and paid
		let mut f = base();
		f.today = date(20, 12);
		f.invoice_number = Some(String::from("R2026-040"));
		f.invoice_date = date(21, 11);
		f.invoice_pdf = true;
		let l = lifecycle(&f);
		assert_eq!(
			marks(&l),
			[Implied, Implied, Done, Implied, Done, Current, Open]
		);
		// the event alone also means the offer was accepted
		let mut f = base();
		f.offer_pdf = true;
		f.today = date(20, 11);
		assert_eq!(marks(&lifecycle(&f))[..4], [Done, Implied, Done, Current]);
	}

	#[test]
	fn canceled_projects_go_to_the_archive() {
		let mut f = base();
		f.offer_pdf = true;
		f.canceled = true;
		let l = lifecycle(&f);
		assert_eq!(
			marks(&l),
			[Done, Skipped, Skipped, Skipped, Skipped, Skipped, Current]
		);
	}
}
