//! Forms: fields with a kind (text, number, date, choice, toggle), errors at the field, Save and Cancel.

use std::path::PathBuf;

use chrono::prelude::*;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::App;
use super::dialogs::Modal;
use super::ops::Op;
use super::text_input::TextInput;
use super::yaml_edit::{self, Kind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    /// a whole number
    Number,
    /// a decimal number like 2,50
    Decimal,
    Date,
    /// one of the options, cycled with ←/→ or Space
    Choice(Vec<String>),
    /// yes or no, Space switches
    Toggle,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub label: String,
    pub input: TextInput,
    pub kind: FieldKind,
    /// selected option of a choice
    pub choice: usize,
    /// state of a toggle
    pub on: bool,
    /// marked with `*`, must not be empty
    pub required: bool,
    /// not filled in yet, marked and focused first
    pub missing: bool,
    pub hint: String,
    pub error: Option<String>,
    /// what the field is: a YAML path for project fields
    pub key: &'static str,
    /// the value when the form opened
    pub original: String,
    /// why a toggle cannot be switched
    pub locked: Option<String>,
}

impl Field {
    pub fn new(key: &'static str, label: String, kind: FieldKind, value: impl Into<String>, hint: String) -> Self {
        let value = value.into();
        Field {
            label,
            input: TextInput::new(value.clone()),
            kind,
            choice: 0,
            on: false,
            required: false,
            missing: false,
            hint,
            error: None,
            key,
            original: value,
            locked: None,
        }
    }

    pub fn choice(key: &'static str, label: String, options: Vec<String>, selected: usize, hint: String) -> Self {
        let mut field = Field::new(key, label, FieldKind::Choice(options), "", hint);
        field.choice = selected;
        field
    }

    pub fn toggle(key: &'static str, label: String, on: bool, hint: String) -> Self {
        let mut field = Field::new(key, label, FieldKind::Toggle, "", hint);
        field.on = on;
        field
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// The value as text: the input, the chosen option, or `yes`/`no`.
    pub fn value(&self) -> String {
        match &self.kind {
            FieldKind::Choice(options) => options.get(self.choice).cloned().unwrap_or_default(),
            FieldKind::Toggle => String::from(if self.on { "yes" } else { "no" }),
            _ => self.input.text().trim().to_owned(),
        }
    }

    pub fn is_text(&self) -> bool {
        !matches!(self.kind, FieldKind::Choice(_) | FieldKind::Toggle)
    }

    /// ←/→ (`step` ±1) or Space (`step` 1) on a choice or toggle.
    pub fn cycle(&mut self, step: isize) {
        match &self.kind {
            FieldKind::Choice(options) if !options.is_empty() => {
                let n = options.len() as isize;
                self.choice = (self.choice as isize + step).rem_euclid(n) as usize;
            },
            FieldKind::Toggle if self.locked.is_none() => self.on = !self.on,
            _ => {},
        }
    }
}

#[derive(Debug, Clone)]
pub enum FormPurpose {
    NewProject,
    /// project fields, written where they changed
    Fill { file: PathBuf },
    Archive { dir: PathBuf },
    /// change (`Some`) or add (`None`) a line in the offer editor
    OfferLine { index: Option<usize> },
    /// change (`Some`) or add (`None`) a product of the template
    TemplateProduct { row: Option<super::template_editor::Row> },
}

#[derive(Debug, Clone)]
pub struct Form {
    pub title: String,
    pub note: Option<String>,
    pub fields: Vec<Field>,
    /// a field, or `fields.len()` for the submit button, `fields.len() + 1` for Cancel
    pub focus: usize,
    /// first visible line, for forms taller than the screen
    pub scroll: u16,
    pub purpose: FormPurpose,
    /// label of the submit button
    pub submit: String,
    /// a problem that belongs to no single field
    pub error: Option<String>,
}

impl Form {
    pub fn new(title: String, purpose: FormPurpose, fields: Vec<Field>) -> Self {
        // the first field that still needs a value
        let focus = fields.iter().position(|f| f.missing).unwrap_or(0);
        let mut form = Form {
            title,
            note: None,
            fields,
            focus,
            scroll: 0,
            purpose,
            submit: lformat!("Save"),
            error: None,
        };
        form.set_focus(focus);
        form
    }

    pub fn note(mut self, note: String) -> Self {
        self.note = Some(note);
        self
    }

    pub fn submit_label(mut self, label: String) -> Self {
        self.submit = label;
        self
    }

    pub fn focus_on(mut self, index: usize) -> Self {
        self.set_focus(index.min(self.fields.len()));
        self
    }

    /// Moves the focus to field (or button) `index`; a text field with content gets selected,
    /// so typing replaces it.
    pub fn set_focus(&mut self, index: usize) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.input.unselect();
        }
        self.focus = index.min(self.fields.len() + 1);
        if let Some(field) = self.fields.get_mut(self.focus) {
            field.input.select_all();
        }
    }

    pub fn submit_focus(&self) -> usize {
        self.fields.len()
    }

    pub fn cancel_focus(&self) -> usize {
        self.fields.len() + 1
    }

    pub fn value(&self, key: &str) -> String {
        self.fields.iter().find(|f| f.key == key).map(Field::value).unwrap_or_default()
    }

    fn field_index(&self, key: &str) -> usize {
        self.fields.iter().position(|f| f.key == key).unwrap_or(0)
    }

    /// Shows `message` at the field `key` and moves there.
    pub fn error_at(&mut self, key: &str, message: String) {
        let index = self.field_index(key);
        if let Some(field) = self.fields.get_mut(index) {
            field.error = Some(message);
        }
        self.set_focus(index);
    }

    fn clear_errors(&mut self) {
        self.error = None;
        for field in &mut self.fields {
            field.error = None;
        }
    }

    /// Basic checks of every field: required, numbers, dates. Dates are normalised to `DD.MM.YYYY`.
    pub fn check(&mut self, today: Date<Utc>) -> bool {
        self.clear_errors();
        let mut first = None;
        for (index, field) in self.fields.iter_mut().enumerate() {
            let value = field.value();
            let error = if value.is_empty() {
                field.required.then(|| lformat!("please fill this in"))
            } else {
                match field.kind {
                    FieldKind::Number if value.parse::<u64>().is_err() => Some(lformat!("please enter a whole number like 42")),
                    FieldKind::Decimal if super::offer::parse_number(&value).is_none_or(|n| n < 0.0) => {
                        Some(lformat!("please enter a number like 2,50"))
                    },
                    FieldKind::Date => match normalize_date(&value, today.year()) {
                        Some(date) => {
                            field.input.set(date);
                            None
                        },
                        None => Some(lformat!("please enter a date like 24.12.2026")),
                    },
                    _ if field.key == "client/title" => title_problem(&value),
                    _ => None,
                }
            };
            if error.is_some() && first.is_none() {
                first = Some(index);
            }
            field.error = error;
        }
        if let Some(index) = first {
            self.set_focus(index);
        }
        first.is_none()
    }
}

/// `24.12.2026`, `24.12.26`, `24.12.` and `24.12` (this year) as `24.12.2026`; `None` if it is no date.
pub fn normalize_date(text: &str, this_year: i32) -> Option<String> {
    let parts: Vec<&str> = text.trim().split('.').map(str::trim).collect();
    let (day, month, year) = match parts.as_slice() {
        [day, month] | [day, month, ""] => (*day, *month, this_year),
        [day, month, year] if year.len() == 2 => (*day, *month, 2000 + year.parse::<i32>().ok()?),
        [day, month, year] if year.len() == 4 => (*day, *month, year.parse::<i32>().ok()?),
        _ => return None,
    };
    let date = NaiveDate::from_ymd_opt(year, month.parse().ok()?, day.parse().ok()?)?;
    Some(date.format("%d.%m.%Y").to_string())
}

/// `HH:MM` with valid hours and minutes.
pub fn is_time(text: &str) -> bool {
    let Some((h, m)) = text.trim().split_once(':') else { return false };
    (1..=2).contains(&h.len())
        && m.len() == 2
        && matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
}

impl App {
    pub fn on_form_key(&mut self, mut form: Form, key: &KeyEvent) -> Option<Modal> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let submit = form.submit_focus();
        let cancel = form.cancel_focus();
        let on_field = form.focus < submit;
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Char('s') if ctrl => return self.submit_form(form),
            KeyCode::Enter if form.focus == cancel => return None,
            KeyCode::Enter => return self.submit_form(form),
            KeyCode::Tab => form.set_focus((form.focus + 1) % (cancel + 1)),
            KeyCode::BackTab => form.set_focus((form.focus + cancel) % (cancel + 1)),
            KeyCode::Down => form.set_focus((form.focus + 1).min(cancel)),
            KeyCode::Up => form.set_focus(form.focus.saturating_sub(1)),
            KeyCode::Left | KeyCode::Right if !on_field => form.set_focus(if form.focus == submit { cancel } else { submit }),
            KeyCode::Char(' ') if !on_field => {
                return if form.focus == cancel { None } else { self.submit_form(form) };
            },
            _ if on_field => {
                let field = &mut form.fields[form.focus];
                match (&field.kind, key.code) {
                    (FieldKind::Choice(_) | FieldKind::Toggle, KeyCode::Left) => field.cycle(-1),
                    (FieldKind::Choice(_) | FieldKind::Toggle, KeyCode::Right | KeyCode::Char(' ')) => field.cycle(1),
                    (FieldKind::Toggle, KeyCode::Char('y' | 'j')) => field.on = field.locked.is_none(),
                    (FieldKind::Toggle, KeyCode::Char('n')) => field.on = false,
                    (FieldKind::Choice(_) | FieldKind::Toggle, _) => {},
                    _ => {
                        if field.input.on_key(key) {
                            field.error = None;
                        }
                    },
                }
                if let Some(reason) = form.fields[form.focus].locked.clone().filter(|_| key.code == KeyCode::Char(' ')) {
                    self.notify(reason);
                }
            },
            _ => {},
        }
        Some(Modal::Form(form))
    }

    /// Checks the form and starts what it is for; on problems the form stays open with the error at the field.
    pub fn submit_form(&mut self, mut form: Form) -> Option<Modal> {
        if !form.check(self.today) {
            return Some(Modal::Form(form));
        }
        let result: Result<Option<Op>, (String, String)> = match form.purpose.clone() {
            FormPurpose::OfferLine { index } => self.submit_offer_line(&form, index).map(|()| None),
            FormPurpose::TemplateProduct { row } => self.submit_template_product(&form, row).map(|()| None),
            FormPurpose::Archive { dir } => match form.value("year").parse::<i32>() {
                Ok(year) if (1900..=9999).contains(&year) => Ok(Some(Op::Archive { dir, year })),
                _ => Err((String::from("year"), lformat!("please enter a year like 2026"))),
            },
            FormPurpose::Fill { file } => self.fill_op(&form, file),
            FormPurpose::NewProject => self.new_project_op(&form),
        };
        match result {
            Ok(op) => {
                if op.is_some() {
                    self.pending = op;
                }
                None
            },
            Err((key, message)) => {
                if key.is_empty() {
                    form.error = Some(message);
                } else {
                    form.error_at(&key, message);
                }
                Some(Modal::Form(form))
            },
        }
    }

    /// The fields that changed, as one write. Empty fields cannot be emptied here.
    fn fill_op(&self, form: &Form, file: PathBuf) -> Result<Option<Op>, (String, String)> {
        let mut values = Vec::new();
        for field in &form.fields {
            let value = field.value();
            if value == field.original.trim() {
                continue;
            }
            if value.is_empty() {
                return Err((field.key.to_owned(), lformat!("cannot be emptied here, use Edit file")));
            }
            if field.key == "invoice/number" {
                if let Some(problem) = self.invoice_number_problem(&file, &value) {
                    return Err((field.key.to_owned(), problem));
                }
            }
            values.push((field.key.to_owned(), value));
        }
        if values.is_empty() {
            return Err((String::new(), lformat!("nothing changed; Esc closes the form")));
        }
        Ok(Some(Op::FillFields { file, values }))
    }

    fn new_project_op(&self, form: &Form) -> Result<Option<Op>, (String, String)> {
        let name = form.value("name");
        let template = form.value("template");
        for key in ["start", "end"] {
            let time = form.value(key);
            if !time.is_empty() && !is_time(&time) {
                return Err((key.to_owned(), lformat!("please enter a time like 18:30")));
            }
        }
        let known = self.data.as_ref().map(|d| d.templates.clone()).unwrap_or_default();
        if !known.is_empty() && !known.contains(&template) {
            return Err((String::from("template"), lformat!("unknown template, available: {}", known.join(", "))));
        }
        if self
            .data
            .as_ref()
            .is_some_and(|d| d.entries.iter().any(|e| e.location == super::data::Location::Working && e.summary.name.eq_ignore_ascii_case(&name)))
        {
            return Err((String::from("name"), lformat!("a running project is already called \"{}\"", name)));
        }
        let mut fill = Vec::new();
        for (key, placeholder) in [
            ("date", "DATE-EVENT"),
            ("start", "TIME-START"),
            ("end", "TIME-END"),
            ("manager", "MANAGER"),
            ("description", "DESCRIPTION"),
        ] {
            let value = form.value(key);
            if !value.is_empty() {
                fill.push((placeholder, value));
            }
        }
        Ok(Some(Op::NewProject {
            name,
            template,
            fill,
            edit: form.value("edit") == "yes",
        }))
    }
}

/// The form field kind for a project field.
pub fn kind_of(kind: Kind) -> FieldKind {
    match kind {
        Kind::Number => FieldKind::Number,
        Kind::Date => FieldKind::Date,
        Kind::Text => FieldKind::Text,
    }
}

/// The hint below a project field.
/// The words a client title can start with: the keys of `gender_matches` in the configuration, capitalised.
pub fn client_titles() -> Vec<String> {
    let mut titles: Vec<String> = asciii::CONFIG
        .get("gender_matches")
        .and_then(|y| y.as_hash())
        .map(|hash| hash.keys().filter_map(|k| k.as_str()).map(capitalise).collect())
        .unwrap_or_default();
    titles.dedup();
    titles
}

fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Why the salutation cannot be made from `title`, like the library does it: the first word has to be
/// in `gender_matches`, and `lang_addressing` needs a phrase for that gender.
pub fn title_problem(title: &str) -> Option<String> {
    let word = title.split_whitespace().next()?;
    let known = || client_titles().join(", ");
    let Some(gender) = asciii::CONFIG.get_str_or(&format!("gender_matches/{}", word.to_lowercase())) else {
        return Some(lformat!("\"{}\" is not a known title; start with one of: {}", word, known()));
    };
    let lang = asciii::CONFIG.get_str("defaults/lang").to_lowercase();
    if asciii::CONFIG.get_str_or(&format!("lang_addressing/{}/{}", lang, gender)).is_none() {
        return Some(lformat!("the configuration has no salutation for \"{}\" (lang_addressing)", word));
    }
    None
}

pub fn hint_for(kind: Kind) -> String {
    match kind {
        Kind::Number => lformat!("a whole number like 42"),
        Kind::Date => lformat!("a date like 24.12.2026, also 24.12.26 or 24.12."),
        Kind::Text => String::new(),
    }
}

/// The project fields of `content` as form fields: human labels, current values, empty ones marked.
pub fn project_fields(content: &str) -> Vec<Field> {
    yaml_edit::fields_in(content)
        .into_iter()
        .map(|(path, kind, value)| {
            let hint = if path == "client/title" {
                lformat!("starts with {}; the salutation comes from it", client_titles().join(", "))
            } else {
                hint_for(kind)
            };
            let mut field = Field::new(path, yaml_edit::label(path), kind_of(kind), value.clone().unwrap_or_default(), hint);
            field.missing = value.is_none();
            field
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_short_forms() {
        assert_eq!(normalize_date("24.12.2026", 2026).as_deref(), Some("24.12.2026"));
        assert_eq!(normalize_date("24.12.26", 2026).as_deref(), Some("24.12.2026"));
        assert_eq!(normalize_date("24.12.", 2027).as_deref(), Some("24.12.2027"));
        assert_eq!(normalize_date("1.2", 2026).as_deref(), Some("01.02.2026"));
        assert_eq!(normalize_date(" 3.4.2026 ", 2026).as_deref(), Some("03.04.2026"));
        assert_eq!(normalize_date("31.02.2026", 2026), None);
        assert_eq!(normalize_date("2026-12-24", 2026), None);
        assert_eq!(normalize_date("24.12.202", 2026), None);
        assert_eq!(normalize_date("", 2026), None);
    }

    #[test]
    fn times() {
        assert!(is_time("18:30"));
        assert!(is_time("8:05"));
        assert!(!is_time("25:00"));
        assert!(!is_time("18.30"));
    }

    fn today() -> Date<Utc> {
        Utc.ymd(2026, 11, 12)
    }

    #[test]
    fn validation_marks_the_field_and_moves_there() {
        let mut form = Form::new(
            String::from("t"),
            FormPurpose::NewProject,
            vec![
                Field::new("name", String::from("Name"), FieldKind::Text, "Sommerball", String::new()).required(),
                Field::new("date", String::from("Date"), FieldKind::Date, "12.12.", String::new()),
                Field::new("number", String::from("No."), FieldKind::Number, "4x", String::new()),
            ],
        );
        assert!(!form.check(today()));
        assert_eq!(form.focus, 2);
        assert!(form.fields[2].error.is_some());
        assert!(form.fields[0].error.is_none());
        // the date was completed
        assert_eq!(form.value("date"), "12.12.2026");

        form.fields[2].input.set("4");
        form.fields[0].input.clear();
        assert!(!form.check(today()));
        assert_eq!(form.focus, 0);
        form.fields[0].input.set("x");
        assert!(form.check(today()));
    }

    #[test]
    fn choices_and_toggles_cycle() {
        let mut choice = Field::choice("t", String::from("T"), vec![String::from("a"), String::from("b")], 0, String::new());
        choice.cycle(-1);
        assert_eq!(choice.value(), "b");
        choice.cycle(1);
        assert_eq!(choice.value(), "a");
        let mut toggle = Field::toggle("e", String::from("E"), false, String::new());
        toggle.cycle(1);
        assert_eq!(toggle.value(), "yes");
        toggle.locked = Some(String::from("no editor"));
        toggle.cycle(1);
        assert_eq!(toggle.value(), "yes");
    }

    #[test]
    fn project_fields_have_labels_and_marks() {
        let content = "client:\n  title: Frau\n  last_name:\ninvoice:\n  number: ##INVOICE-NUMBER##\n  date: 01.12.2026\n";
        let fields = project_fields(content);
        let names: Vec<&str> = fields.iter().map(|f| f.key).collect();
        assert_eq!(names, ["client/title", "client/last_name", "invoice/number", "invoice/date"]);
        assert!(!fields[0].missing && fields[1].missing && fields[2].missing && !fields[3].missing);
        assert!(fields.iter().all(|f| !f.label.contains('/')));
        let form = Form::new(String::new(), FormPurpose::Fill { file: PathBuf::new() }, fields);
        // the first empty field has the focus
        assert_eq!(form.focus, 1);
    }

    #[test]
    fn a_prefilled_field_is_selected_when_it_gets_the_focus() {
        use ratatui::crossterm::event::KeyEventKind;
        let key = |c| KeyEvent::new_with_kind(KeyCode::Char(c), KeyModifiers::NONE, KeyEventKind::Press);
        let fields = vec![
            Field::new("invoice/payed_date", String::from("Paid on"), FieldKind::Date, "12.11.2026", String::new()),
            Field::new("manager", String::from("Manager"), FieldKind::Text, "Tom", String::new()),
        ];
        let mut form = Form::new(String::new(), FormPurpose::Fill { file: PathBuf::new() }, fields);
        assert!(form.fields[0].input.is_selected());
        for c in "12.11.".chars() {
            form.fields[0].input.on_key(&key(c));
        }
        assert_eq!(form.fields[0].input.text(), "12.11.");
        // Tab: the next field is selected, the one left behind is not
        form.set_focus(1);
        assert!(form.fields[1].input.is_selected());
        assert!(!form.fields[0].input.is_selected());
        // an error moves the focus back and selects the wrong value
        form.error_at("invoice/payed_date", String::from("wrong"));
        assert_eq!(form.focus, 0);
        assert!(form.fields[0].input.is_selected());
    }

    #[test]
    fn client_title_must_give_a_salutation() {
        assert!(client_titles().iter().any(|t| t == "Herr"));
        assert_eq!(title_problem("Herr"), None);
        assert_eq!(title_problem("Frau Dr."), None);
        let problem = title_problem("Hr.").expect("Hr. is no title of gender_matches");
        assert!(problem.contains("Herr"), "{}", problem);
        // a form does not accept it
        let mut form = Form::new(String::new(), FormPurpose::Fill { file: PathBuf::new() }, vec![Field::new(
            "client/title",
            String::from("Client title"),
            FieldKind::Text,
            "Dr",
            String::new(),
        )]);
        assert!(!form.check(Utc::today()));
        assert!(form.fields[0].error.is_some());
    }
}
