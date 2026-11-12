//! ZUGFeRD / Factur-X e-invoices (profile EN 16931).
//!
//! Switched on with `document_export/zugferd: true` in the config. `asciii make` then
//! writes the invoice as XML (CII) next to the PDF and attaches it to the PDF, which becomes a PDF/A-3b.
//!
//! The numbers come from the same bill the PDF is made from. Data that is not in the project file
//! (the seller, bank account, payment terms) comes from a `zugferd:` section in the `.asciii.yml`
//! of the storage directory (so it travels with the projects), or else from the normal config.
//!
//! ```yaml
//! document_export:
//!   zugferd: true
//! zugferd:
//!   seller:
//!     name: Studentencafé ascii
//!     street: Nöthnitzer Straße 46
//!     postcode: "01187"
//!     city: Dresden
//!     vat_id: DE123456789     # and/or tax_number: 203/123/45678
//!     email: kontakt@example.com
//!   payment:
//!     iban: DE02 1203 0000 0000 2020 51
//!     days: 14
//!   exemption_reason: "Steuerfrei nach § 4 Nr. 18 UStG"  # only needed for lines with 0 % VAT
//! ```
//!
//! An optional `client/reference` in a project becomes the buyer reference (Leitweg-ID for public clients).

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Error;
use chrono::{Duration, NaiveDate};
use regex::Regex;
use yaml_rust::Yaml;

use crate::{
    project::{
        spec::{Invoicable, IsClient, IsProject, Redeemable},
        Project,
    },
    storage,
    util::yaml,
};

pub mod pdf;
pub mod xml;

use self::xml::{Address, EInvoice, Line, Party, Payment, TaxSum};

#[derive(thiserror::Error, Debug)]
pub enum ZugferdError {
    #[error("the e-invoice (ZUGFeRD) cannot be made, missing: {}", _0)]
    Incomplete(String),
}

/// Settings from the storage's `.asciii.yml`, falling back to the config.
struct Settings {
    store: Yaml,
}

fn scalar(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) | Yaml::Real(s) => Some(s.trim().to_string()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

impl Settings {
    fn load() -> Self {
        let path = storage::get_storage_path().join(".asciii.yml");
        let store = if path.exists() {
            yaml::open(&path).unwrap_or_else(|e| {
                log::warn!("cannot read {}: {}", path.display(), e);
                Yaml::Null
            })
        } else {
            Yaml::Null
        };
        Settings { store }
    }

    fn raw(&self, path: &str) -> Option<&Yaml> {
        yaml::get(&self.store, path).or_else(|| crate::CONFIG.get(path))
    }

    fn get(&self, key: &str) -> Option<String> {
        self.raw(&format!("zugferd/{}", key)).and_then(scalar)
    }

    fn enabled(&self) -> bool {
        self.raw("document_export/zugferd")
            .and_then(Yaml::as_bool)
            .unwrap_or(false)
    }
}

/// Is `document_export/zugferd` switched on (storage `.asciii.yml` or config)?
pub fn enabled() -> bool {
    Settings::load().enabled()
}

/// German postcodes have 5 digits; YAML turns an unquoted `01187` into 1187.
fn postcode(code: String, country: &str) -> String {
    if country == "DE" && code.len() < 5 && code.chars().all(|c| c.is_ascii_digit()) {
        format!("{:0>5}", code)
    } else {
        code
    }
}

fn country_code(text: &str) -> Option<String> {
    let lower = text.trim().trim_end_matches('.').to_lowercase();
    let code = match lower.as_str() {
        "deutschland" | "germany" | "brd" => "DE",
        "österreich" | "oesterreich" | "austria" => "AT",
        "schweiz" | "switzerland" | "suisse" => "CH",
        "frankreich" | "france" => "FR",
        "niederlande" | "netherlands" | "nederland" => "NL",
        "belgien" | "belgium" => "BE",
        "polen" | "poland" | "polska" => "PL",
        "tschechien" | "czechia" | "czech republic" => "CZ",
        "dänemark" | "denmark" => "DK",
        "luxemburg" | "luxembourg" => "LU",
        "italien" | "italy" => "IT",
        "spanien" | "spain" => "ES",
        "vereinigtes königreich" | "united kingdom" | "uk" | "england" => "GB",
        _ if text.trim().len() == 2 && text.trim().chars().all(|c| c.is_ascii_uppercase()) => text.trim(),
        _ => return None,
    };
    Some(code.to_string())
}

/// Splits a free text address (as written in `client/address`) into name and postal address.
///
/// ```text
/// TU Dresden            ← name
/// Fakultät Informatik   ← further line
/// Nöthnitzer Str. 46    ← street (the line before the postcode)
/// 01187 Dresden         ← postcode and city
/// Deutschland           ← optional country
/// ```
pub fn split_address(text: &str, default_country: &str) -> Result<(String, Address), String> {
    lazy_static::lazy_static! {
        static ref POSTCODE: Regex = Regex::new(r"^(?:([A-Z]{1,2})\s*-\s*)?(\d{4,5})\s+(\S.*)$").unwrap();
    }
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let index = lines
        .iter()
        .rposition(|line| POSTCODE.is_match(line))
        .ok_or_else(|| lformat!("client address: no line with postcode and city"))?;
    if index == 0 {
        return Err(lformat!("client address: the first line has to be the name"));
    }
    let captures = POSTCODE.captures(lines[index]).unwrap();
    let mut country = match captures.get(1).map(|m| m.as_str()) {
        Some("D") => "DE".to_string(),
        Some("A") => "AT".to_string(),
        Some(prefix) if prefix.len() == 2 => prefix.to_string(),
        _ => default_country.to_string(),
    };
    match lines[index + 1..] {
        [] => {}
        [last] => {
            country = country_code(last).ok_or_else(|| lformat!("client address: unknown country {}", last))?;
        }
        _ => return Err(lformat!("client address: more than one line after postcode and city")),
    }
    let street = (index >= 2).then(|| lines[index - 1].to_string());
    let extra = (index >= 3).then(|| lines[1..index - 1].join(", "));
    Ok((
        lines[0].to_string(),
        Address {
            street,
            extra,
            postcode: captures[2].to_string(),
            city: captures[3].trim().to_string(),
            country,
        },
    ))
}

fn unit_code(unit: Option<&str>) -> &'static str {
    match unit.map(|u| u.trim().trim_end_matches('.').to_lowercase()).as_deref() {
        Some("h" | "std" | "stunde" | "stunden" | "hour" | "hours") => "HUR",
        _ => "C62",
    }
}

fn line_name(name: &str, unit: Option<&str>) -> String {
    match unit.map(str::trim) {
        Some(unit)
            if !unit.is_empty()
                && unit_code(Some(unit)) == "C62"
                && !matches!(unit.to_lowercase().as_str(), "stk" | "stk." | "stück" | "st" | "pcs" | "x") =>
        {
            format!("{} ({})", name, unit)
        }
        _ => name.to_string(),
    }
}

fn naive(date: chrono::Date<chrono::Utc>) -> NaiveDate {
    date.naive_utc()
}

/// Collects the e-invoice; returns what is missing instead if it cannot be made.
pub fn collect(project: &Project) -> Result<EInvoice, Vec<String>> {
    let settings = Settings::load();
    collect_with(project, &settings)
}

fn collect_with(project: &Project, settings: &Settings) -> Result<EInvoice, Vec<String>> {
    let mut missing = Vec::new();
    let mut require = |key: &str| {
        let value = settings.get(key);
        if value.is_none() {
            missing.push(lformat!("zugferd/{} in the config", key));
        }
        value.unwrap_or_default()
    };

    let seller_country = settings.get("seller/country").unwrap_or_else(|| "DE".into());
    let seller_name = require("seller/name");
    let seller_street = require("seller/street");
    let seller_postcode = postcode(require("seller/postcode"), &seller_country);
    let seller_city = require("seller/city");
    let iban = require("payment/iban");
    let vat_id = settings.get("seller/vat_id");
    let tax_number = settings.get("seller/tax_number");
    if vat_id.is_none() && tax_number.is_none() {
        missing.push(lformat!("zugferd/seller/vat_id or zugferd/seller/tax_number in the config"));
    }
    let seller_email = settings.get("seller/email");
    let seller = Party {
        name: seller_name.clone(),
        contact: settings.get("seller/contact"),
        phone: settings.get("seller/phone"),
        contact_email: seller_email.clone(),
        address: Address {
            street: Some(seller_street),
            extra: None,
            postcode: seller_postcode,
            city: seller_city,
            country: seller_country.clone(),
        },
        email: seller_email,
        vat_id,
        tax_number,
    };

    let invoice = project.invoice();
    let number = invoice.number_long_str();
    let issued = invoice.date().ok().map(naive);
    if number.is_none() || issued.is_none() {
        missing.push(lformat!("invoice number and date"));
    }

    let client = project.client();
    let buyer_country = settings.get("buyer_country").unwrap_or_else(|| "DE".into());
    let buyer = match client.address() {
        Ok(text) => match split_address(text, &buyer_country) {
            Ok((name, address)) => Some(Party {
                contact: client.full_name().filter(|full| full.trim() != name),
                name,
                address,
                email: client.email().ok().map(str::to_string).filter(|m| m.contains('@')),
                ..Party::default()
            }),
            Err(problem) => {
                missing.push(problem);
                None
            }
        },
        Err(_) => {
            missing.push(lformat!("client address"));
            None
        }
    };

    let mut lines = Vec::new();
    let mut taxes = Vec::new();
    let (mut net, mut tax, mut gross) = (0, 0, 0);
    match project.bills() {
        Ok((_, bill)) => {
            for (rate, item) in bill.as_items_with_tax() {
                lines.push(Line {
                    name: line_name(item.product.name, item.product.unit),
                    quantity: item.amount,
                    unit_code: unit_code(item.product.unit),
                    price: item.product.price.value,
                    total: item.gross().value,
                    rate: rate.value(),
                });
            }
            for (rate, items) in bill.iter().rev() {
                taxes.push(TaxSum {
                    rate: rate.value(),
                    basis: items.gross_sum().value,
                    tax: items.tax_sum().value,
                });
            }
            net = bill.gross_total().value;
            tax = bill.tax_total().value;
            gross = bill.net_total().value;
        }
        Err(e) => missing.push(lformat!("a valid bill ({})", e)),
    }
    if lines.is_empty() && missing.is_empty() {
        missing.push(lformat!("invoice items"));
    }
    let exemption_reason = settings.get("exemption_reason");
    if exemption_reason.is_none() && lines.iter().any(|l| xml::category(l.rate) == "E") {
        missing.push(lformat!("zugferd/exemption_reason in the config (there are items without VAT)"));
    }

    if !missing.is_empty() {
        return Err(missing);
    }
    let issued = issued.unwrap();
    let days = settings
        .get("payment/days")
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(14);

    Ok(EInvoice {
        number: number.unwrap(),
        issued,
        delivered: project.event_date().ok().map(naive),
        currency: settings.get("currency").unwrap_or_else(|| "EUR".into()),
        buyer_reference: yaml::get_to_string(project.yaml(), "client/reference")
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty()),
        seller,
        buyer: buyer.unwrap(),
        lines,
        taxes,
        net,
        tax,
        gross,
        payment: Payment {
            iban: Some(iban.split_whitespace().collect()),
            bic: settings.get("payment/bic").map(|b| b.split_whitespace().collect()),
            account_name: settings.get("payment/account_name").or(Some(seller_name)),
            terms: settings.get("payment/terms"),
            due: Some(issued + Duration::days(days)),
        },
        exemption_reason,
    })
}

/// What the e-invoice of this project still needs (empty if it can be made).
pub fn missing(project: &Project) -> Vec<String> {
    collect(project).err().unwrap_or_default()
}

/// The `factur-x.xml` of this project.
pub fn invoice_xml(project: &Project) -> Result<String, Error> {
    let invoice = collect(project).map_err(|missing| ZugferdError::Incomplete(missing.join(", ")))?;
    Ok(xml::render(&invoice))
}

/// Writes the XML next to `pdf` and attaches it to the PDF. Returns the path of the XML.
pub fn export(project: &Project, pdf: &Path) -> Result<PathBuf, Error> {
    let xml = invoice_xml(project)?;
    let xml_path = pdf.with_extension("xml");
    fs::write(&xml_path, &xml)?;
    pdf::embed(pdf, xml.as_bytes())?;
    Ok(xml_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        let (name, address) = split_address("Björn Döbel\nNöthnitzer Straße 46\n01187 Dresden\n", "DE").unwrap();
        assert_eq!(name, "Björn Döbel");
        assert_eq!(address.street.as_deref(), Some("Nöthnitzer Straße 46"));
        assert_eq!(address.extra, None);
        assert_eq!(address.postcode, "01187");
        assert_eq!(address.city, "Dresden");
        assert_eq!(address.country, "DE");

        let (name, address) = split_address(
            "TU Dresden\nFakultät Informatik\nHerr Müller\nNöthnitzer Str. 46\nD-01187 Dresden",
            "DE",
        )
        .unwrap();
        assert_eq!(name, "TU Dresden");
        assert_eq!(address.extra.as_deref(), Some("Fakultät Informatik, Herr Müller"));
        assert_eq!(address.street.as_deref(), Some("Nöthnitzer Str. 46"));

        let (_, address) = split_address("Firma\nGasse 1\n1010 Wien\nÖsterreich", "DE").unwrap();
        assert_eq!((address.postcode.as_str(), address.country.as_str()), ("1010", "AT"));

        let (_, address) = split_address("Firma\n01069 Dresden", "DE").unwrap();
        assert_eq!(address.street, None);

        assert!(split_address("Björn Döbel\nNöthnitzer Straße 46", "DE").is_err());
        assert!(split_address("01187 Dresden", "DE").is_err());
        assert!(split_address("Firma\nGasse 1\n1010 Wien\nAtlantis", "DE").is_err());
    }

    #[test]
    fn units_and_codes() {
        assert_eq!(unit_code(Some("h")), "HUR");
        assert_eq!(unit_code(Some("Std.")), "HUR");
        assert_eq!(unit_code(Some("1l")), "C62");
        assert_eq!(unit_code(None), "C62");
        assert_eq!(line_name("Kaffee", Some("1l")), "Kaffee (1l)");
        assert_eq!(line_name("Brötchen", Some("stk")), "Brötchen");
        assert_eq!(line_name("Service", Some("h")), "Service");
        assert_eq!(postcode("1187".into(), "DE"), "01187");
        assert_eq!(postcode("1010".into(), "AT"), "1010");
        assert_eq!(country_code("Deutschland").as_deref(), Some("DE"));
        assert_eq!(country_code("NL").as_deref(), Some("NL"));
    }

    fn settings(text: &str) -> Settings {
        Settings {
            store: yaml::parse(text).unwrap(),
        }
    }

    const SELLER: &str = r#"
document_export:
  zugferd: true
zugferd:
  seller:
    name: Café ascii
    street: Nöthnitzer Str. 46
    postcode: 01187
    city: Dresden
    vat_id: DE123456789
  payment:
    iban: DE02 1203 0000 0000 2020 51
    days: 10
  exemption_reason: steuerfrei
"#;

    const PROJECT: &str = r#"
format: 2.4.0
client:
  title: Herr
  first_name: Björn
  last_name: Döbel
  email: b@example.com
  reference: 04011000-1234512345-06
  address: |
    Firma GmbH
    Hauptstr. 1
    01069 Dresden
event:
  name: Test
  dates:
  - begin: 15.01.2026
offer:
  date: 01.01.2026
  appendix: 1
invoice:
  number: 30
  date: 20.01.2026
cataloge:
  product: &kaffee { name: Kaffee, price: 2.5, unit: 1l }
  product: &tee { name: Tee, price: 1.75, unit: 1l, tax: 0.07 }
products:
  *kaffee: { amount: 8 }
  *tee: { amount: 4, returned: 1 }
hours:
  salary: 8.50
  caterers:
    Hans: 2.5
"#;

    #[test]
    fn invoice_from_project() {
        let project = Project::from_file_content(PROJECT).unwrap();
        let invoice = collect_with(&project, &settings(SELLER)).unwrap();
        assert_eq!(invoice.number, "R2026-030");
        assert_eq!(invoice.seller.address.postcode, "01187");
        assert_eq!(invoice.payment.iban.as_deref(), Some("DE02120300000000202051"));
        assert_eq!(invoice.payment.due, NaiveDate::from_ymd_opt(2026, 1, 30));
        assert_eq!(invoice.delivered, NaiveDate::from_ymd_opt(2026, 1, 15));
        assert_eq!(invoice.buyer.name, "Firma GmbH");
        assert_eq!(invoice.buyer.contact.as_deref(), Some("Björn Döbel"));
        assert_eq!(invoice.buyer_reference.as_deref(), Some("04011000-1234512345-06"));

        // the same numbers as the bill the PDF is made from
        let (_, bill) = project.bills().unwrap();
        assert_eq!(invoice.net, bill.gross_total().value);
        assert_eq!(invoice.gross, bill.net_total().value);
        assert_eq!(invoice.lines.iter().map(|l| l.total).sum::<i64>(), invoice.net);
        assert_eq!(invoice.taxes.iter().map(|t| t.basis).sum::<i64>(), invoice.net);
        assert_eq!(invoice.taxes.iter().map(|t| t.tax).sum::<i64>(), invoice.tax);
        assert_eq!(invoice.net + invoice.tax, invoice.gross);
        // 8 × 2.50 + 3 × 1.75 + 2.5 h × 8.50
        assert_eq!(invoice.net, 2000 + 525 + 2125);
        let service = invoice.lines.iter().find(|l| l.unit_code == "HUR").unwrap();
        assert_eq!(service.rate, 0.0);
        assert_eq!(invoice.taxes.len(), 3);

        let xml = xml::render(&invoice);
        assert!(xml.contains("<ram:BuyerReference>04011000-1234512345-06</ram:BuyerReference>"));
    }

    /// `ASCIII_ZUGFERD=dir cargo test --lib zugferd_real_files -- --ignored --nocapture`:
    /// every project file below `dir` with an invoice gives XML or a list of what is missing, no panics
    #[test]
    #[ignore]
    fn zugferd_real_files() {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() && !path.ends_with("target") && !path.ends_with(".git") {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "yml") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(Path::new(&std::env::var("ASCIII_ZUGFERD").expect("set ASCIII_ZUGFERD")), &mut files);
        let (mut ok, mut problems) = (0, 0);
        for file in files {
            let Ok(content) = fs::read_to_string(&file) else { continue };
            if yaml::parse(&content).is_err() {
                continue;
            }
            let Ok(project) = Project::from_file_content(&content) else { continue };
            if project.invoice().number().is_err() {
                continue;
            }
            match collect_with(&project, &settings(SELLER)) {
                Ok(invoice) => {
                    let xml = xml::render(&invoice);
                    assert_eq!(xml.matches("<ram:").count(), xml.matches("</ram:").count());
                    assert_eq!(invoice.lines.iter().map(|l| l.total).sum::<i64>(), invoice.net);
                    ok += 1;
                },
                Err(missing) => {
                    println!("{}: {}", file.display(), missing.join(", "));
                    problems += 1;
                },
            }
        }
        println!("{} ok, {} with problems", ok, problems);
    }

    #[test]
    fn reports_what_is_missing() {
        let project = Project::from_file_content(PROJECT).unwrap();
        let missing = collect_with(&project, &settings("zugferd:\n  seller:\n    name: X\n")).unwrap_err();
        let text = missing.join("|");
        assert!(text.contains("zugferd/seller/street"), "{}", text);
        assert!(text.contains("zugferd/payment/iban"), "{}", text);
        assert!(text.contains("vat_id"), "{}", text);
        assert!(text.contains("exemption_reason"), "{}", text);
        assert!(!text.contains("seller/name"), "{}", text);
    }
}
