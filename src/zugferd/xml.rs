//! Writes the invoice as UN/CEFACT Cross Industry Invoice (CII D16B), profile EN 16931.
//!
//! The element order follows the CII schema; validators reject other orders.

use std::fmt::Write;

use chrono::NaiveDate;

/// Guideline ID of the EN 16931 profile (ZUGFeRD 2.x "COMFORT", Factur-X "EN 16931").
pub const PROFILE: &str = "urn:cen.eu:en16931:2017";

/// A postal address as CII wants it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Address {
    pub street: Option<String>,
    /// further address lines (department, c/o, …)
    pub extra: Option<String>,
    pub postcode: String,
    pub city: String,
    /// ISO 3166-1 alpha-2
    pub country: String,
}

/// Seller or buyer.
#[derive(Debug, Clone, Default)]
pub struct Party {
    pub name: String,
    pub contact: Option<String>,
    pub phone: Option<String>,
    pub contact_email: Option<String>,
    pub address: Address,
    /// electronic address (BT-34 / BT-49), an e-mail address
    pub email: Option<String>,
    /// USt-IdNr.
    pub vat_id: Option<String>,
    /// Steuernummer
    pub tax_number: Option<String>,
}

/// One invoice line, amounts in cents.
#[derive(Debug, Clone)]
pub struct Line {
    pub name: String,
    pub quantity: f64,
    /// UN/ECE Rec 20 code, e.g. `C62` (piece) or `HUR` (hour)
    pub unit_code: &'static str,
    pub price: i64,
    pub total: i64,
    /// 0.19 for 19 %
    pub rate: f64,
}

/// Totals for one VAT rate, amounts in cents.
#[derive(Debug, Clone)]
pub struct TaxSum {
    pub rate: f64,
    pub basis: i64,
    pub tax: i64,
}

#[derive(Debug, Clone)]
pub struct Payment {
    pub iban: Option<String>,
    pub bic: Option<String>,
    pub account_name: Option<String>,
    pub terms: Option<String>,
    pub due: Option<NaiveDate>,
}

/// Everything that goes into the XML.
#[derive(Debug, Clone)]
pub struct EInvoice {
    pub number: String,
    pub issued: NaiveDate,
    pub delivered: Option<NaiveDate>,
    pub currency: String,
    pub buyer_reference: Option<String>,
    pub seller: Party,
    pub buyer: Party,
    pub lines: Vec<Line>,
    pub taxes: Vec<TaxSum>,
    /// sum of the line totals (= tax basis, there are no allowances or charges)
    pub net: i64,
    pub tax: i64,
    pub gross: i64,
    pub payment: Payment,
    /// required for 0 % lines (VAT category E)
    pub exemption_reason: Option<String>,
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // control characters are not allowed in XML 1.0
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// cents as `12.34`
pub fn amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    format!("{}{}.{:02}", sign, cents.abs() / 100, cents.abs() % 100)
}

/// a number without trailing zeros, at most `decimals` decimals
pub fn decimal(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value);
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    };
    if text == "-0" {
        "0".into()
    } else {
        text
    }
}

/// 0.19 → `19`
pub fn percent(rate: f64) -> String {
    decimal(rate * 100.0, 2)
}

/// VAT category: S (standard/reduced rate) or E (exempt)
pub fn category(rate: f64) -> &'static str {
    if rate.abs() < 1e-9 {
        "E"
    } else {
        "S"
    }
}

fn date(date: NaiveDate) -> String {
    format!(
        "<udt:DateTimeString format=\"102\">{}</udt:DateTimeString>",
        date.format("%Y%m%d")
    )
}

struct Xml {
    out: String,
    depth: usize,
}

impl Xml {
    fn open(&mut self, tag: &str) {
        let _ = writeln!(self.out, "{:indent$}<{}>", "", tag, indent = self.depth * 2);
        self.depth += 1;
    }

    fn close(&mut self, tag: &str) {
        self.depth -= 1;
        let _ = writeln!(self.out, "{:indent$}</{}>", "", tag, indent = self.depth * 2);
    }

    /// `<tag attrs>text</tag>`, text is escaped
    fn leaf(&mut self, tag: &str, attrs: &str, text: &str) {
        let _ = writeln!(
            self.out,
            "{:indent$}<{}{}>{}</{}>",
            "",
            tag,
            attrs,
            escape(text),
            tag,
            indent = self.depth * 2
        );
    }

    fn opt(&mut self, tag: &str, text: &Option<String>) {
        if let Some(text) = text {
            self.leaf(tag, "", text);
        }
    }

    /// `<tag>` with already formatted inner xml
    fn raw(&mut self, tag: &str, inner: &str) {
        let _ = writeln!(self.out, "{:indent$}<{}>{}</{}>", "", tag, inner, tag, indent = self.depth * 2);
    }

    fn party(&mut self, tag: &str, party: &Party) {
        self.open(tag);
        self.leaf("ram:Name", "", &party.name);
        if party.contact.is_some() || party.phone.is_some() || party.contact_email.is_some() {
            self.open("ram:DefinedTradeContact");
            self.opt("ram:PersonName", &party.contact);
            if let Some(phone) = &party.phone {
                self.open("ram:TelephoneUniversalCommunication");
                self.leaf("ram:CompleteNumber", "", phone);
                self.close("ram:TelephoneUniversalCommunication");
            }
            if let Some(mail) = &party.contact_email {
                self.open("ram:EmailURIUniversalCommunication");
                self.leaf("ram:URIID", "", mail);
                self.close("ram:EmailURIUniversalCommunication");
            }
            self.close("ram:DefinedTradeContact");
        }
        let address = &party.address;
        self.open("ram:PostalTradeAddress");
        self.leaf("ram:PostcodeCode", "", &address.postcode);
        self.opt("ram:LineOne", &address.street);
        self.opt("ram:LineTwo", &address.extra);
        self.leaf("ram:CityName", "", &address.city);
        self.leaf("ram:CountryID", "", &address.country);
        self.close("ram:PostalTradeAddress");
        if let Some(mail) = &party.email {
            self.open("ram:URIUniversalCommunication");
            self.leaf("ram:URIID", " schemeID=\"EM\"", mail);
            self.close("ram:URIUniversalCommunication");
        }
        for (scheme, id) in [("VA", &party.vat_id), ("FC", &party.tax_number)] {
            if let Some(id) = id {
                self.open("ram:SpecifiedTaxRegistration");
                self.leaf("ram:ID", &format!(" schemeID=\"{}\"", scheme), id);
                self.close("ram:SpecifiedTaxRegistration");
            }
        }
        self.close(tag);
    }
}

/// The complete `factur-x.xml`.
pub fn render(invoice: &EInvoice) -> String {
    let mut x = Xml {
        out: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
        depth: 0,
    };
    x.out.push_str(concat!(
        "<rsm:CrossIndustryInvoice",
        " xmlns:rsm=\"urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100\"",
        " xmlns:qdt=\"urn:un:unece:uncefact:data:standard:QualifiedDataType:100\"",
        " xmlns:ram=\"urn:un:unece:uncefact:data:standard:ReusableAggregateBusinessInformationEntity:100\"",
        " xmlns:udt=\"urn:un:unece:uncefact:data:standard:UnqualifiedDataType:100\">\n"
    ));
    x.depth = 1;

    x.open("rsm:ExchangedDocumentContext");
    x.open("ram:GuidelineSpecifiedDocumentContextParameter");
    x.leaf("ram:ID", "", PROFILE);
    x.close("ram:GuidelineSpecifiedDocumentContextParameter");
    x.close("rsm:ExchangedDocumentContext");

    x.open("rsm:ExchangedDocument");
    x.leaf("ram:ID", "", &invoice.number);
    // 380: commercial invoice
    x.leaf("ram:TypeCode", "", "380");
    x.raw("ram:IssueDateTime", &date(invoice.issued));
    x.close("rsm:ExchangedDocument");

    x.open("rsm:SupplyChainTradeTransaction");

    for (index, line) in invoice.lines.iter().enumerate() {
        x.open("ram:IncludedSupplyChainTradeLineItem");
        x.open("ram:AssociatedDocumentLineDocument");
        x.leaf("ram:LineID", "", &(index + 1).to_string());
        x.close("ram:AssociatedDocumentLineDocument");
        x.open("ram:SpecifiedTradeProduct");
        x.leaf("ram:Name", "", &line.name);
        x.close("ram:SpecifiedTradeProduct");
        x.open("ram:SpecifiedLineTradeAgreement");
        x.open("ram:NetPriceProductTradePrice");
        x.leaf("ram:ChargeAmount", "", &amount(line.price));
        x.close("ram:NetPriceProductTradePrice");
        x.close("ram:SpecifiedLineTradeAgreement");
        x.open("ram:SpecifiedLineTradeDelivery");
        x.leaf(
            "ram:BilledQuantity",
            &format!(" unitCode=\"{}\"", line.unit_code),
            &decimal(line.quantity, 4),
        );
        x.close("ram:SpecifiedLineTradeDelivery");
        x.open("ram:SpecifiedLineTradeSettlement");
        x.open("ram:ApplicableTradeTax");
        x.leaf("ram:TypeCode", "", "VAT");
        x.leaf("ram:CategoryCode", "", category(line.rate));
        x.leaf("ram:RateApplicablePercent", "", &percent(line.rate));
        x.close("ram:ApplicableTradeTax");
        x.open("ram:SpecifiedTradeSettlementLineMonetarySummation");
        x.leaf("ram:LineTotalAmount", "", &amount(line.total));
        x.close("ram:SpecifiedTradeSettlementLineMonetarySummation");
        x.close("ram:SpecifiedLineTradeSettlement");
        x.close("ram:IncludedSupplyChainTradeLineItem");
    }

    x.open("ram:ApplicableHeaderTradeAgreement");
    x.opt("ram:BuyerReference", &invoice.buyer_reference);
    x.party("ram:SellerTradeParty", &invoice.seller);
    x.party("ram:BuyerTradeParty", &invoice.buyer);
    x.close("ram:ApplicableHeaderTradeAgreement");

    x.open("ram:ApplicableHeaderTradeDelivery");
    if let Some(delivered) = invoice.delivered {
        x.open("ram:ActualDeliverySupplyChainEvent");
        x.raw("ram:OccurrenceDateTime", &date(delivered));
        x.close("ram:ActualDeliverySupplyChainEvent");
    }
    x.close("ram:ApplicableHeaderTradeDelivery");

    x.open("ram:ApplicableHeaderTradeSettlement");
    x.leaf("ram:PaymentReference", "", &invoice.number);
    x.leaf("ram:InvoiceCurrencyCode", "", &invoice.currency);
    let payment = &invoice.payment;
    if payment.iban.is_some() {
        x.open("ram:SpecifiedTradeSettlementPaymentMeans");
        // 58: SEPA credit transfer
        x.leaf("ram:TypeCode", "", "58");
        x.open("ram:PayeePartyCreditorFinancialAccount");
        x.opt("ram:IBANID", &payment.iban);
        x.opt("ram:AccountName", &payment.account_name);
        x.close("ram:PayeePartyCreditorFinancialAccount");
        if payment.bic.is_some() {
            x.open("ram:PayeeSpecifiedCreditorFinancialInstitution");
            x.opt("ram:BICID", &payment.bic);
            x.close("ram:PayeeSpecifiedCreditorFinancialInstitution");
        }
        x.close("ram:SpecifiedTradeSettlementPaymentMeans");
    }
    for sum in &invoice.taxes {
        x.open("ram:ApplicableTradeTax");
        x.leaf("ram:CalculatedAmount", "", &amount(sum.tax));
        x.leaf("ram:TypeCode", "", "VAT");
        if category(sum.rate) == "E" {
            x.opt("ram:ExemptionReason", &invoice.exemption_reason);
        }
        x.leaf("ram:BasisAmount", "", &amount(sum.basis));
        x.leaf("ram:CategoryCode", "", category(sum.rate));
        x.leaf("ram:RateApplicablePercent", "", &percent(sum.rate));
        x.close("ram:ApplicableTradeTax");
    }
    if payment.terms.is_some() || payment.due.is_some() {
        x.open("ram:SpecifiedTradePaymentTerms");
        x.opt("ram:Description", &payment.terms);
        if let Some(due) = payment.due {
            x.raw("ram:DueDateDateTime", &date(due));
        }
        x.close("ram:SpecifiedTradePaymentTerms");
    }
    x.open("ram:SpecifiedTradeSettlementHeaderMonetarySummation");
    x.leaf("ram:LineTotalAmount", "", &amount(invoice.net));
    x.leaf("ram:TaxBasisTotalAmount", "", &amount(invoice.net));
    x.leaf(
        "ram:TaxTotalAmount",
        &format!(" currencyID=\"{}\"", invoice.currency),
        &amount(invoice.tax),
    );
    x.leaf("ram:GrandTotalAmount", "", &amount(invoice.gross));
    x.leaf("ram:DuePayableAmount", "", &amount(invoice.gross));
    x.close("ram:SpecifiedTradeSettlementHeaderMonetarySummation");
    x.close("ram:ApplicableHeaderTradeSettlement");

    x.close("rsm:SupplyChainTradeTransaction");
    x.out.push_str("</rsm:CrossIndustryInvoice>\n");
    x.out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> EInvoice {
        let address = |street: &str, postcode: &str, city: &str| Address {
            street: Some(street.into()),
            extra: None,
            postcode: postcode.into(),
            city: city.into(),
            country: "DE".into(),
        };
        EInvoice {
            number: "R2026-030".into(),
            issued: NaiveDate::from_ymd_opt(2026, 1, 20).unwrap(),
            delivered: NaiveDate::from_ymd_opt(2026, 1, 15),
            currency: "EUR".into(),
            buyer_reference: None,
            seller: Party {
                name: "Café <ascii> & Co".into(),
                address: address("Nöthnitzer Str. 46", "01187", "Dresden"),
                vat_id: Some("DE123456789".into()),
                ..Party::default()
            },
            buyer: Party {
                name: "Björn Döbel".into(),
                address: address("Hauptstr. 1", "01069", "Dresden"),
                email: Some("b@example.com".into()),
                ..Party::default()
            },
            lines: vec![
                Line {
                    name: "Kaffee (1l)".into(),
                    quantity: 8.0,
                    unit_code: "C62",
                    price: 250,
                    total: 2000,
                    rate: 0.19,
                },
                Line {
                    name: "Service".into(),
                    quantity: 2.5,
                    unit_code: "HUR",
                    price: 850,
                    total: 2125,
                    rate: 0.0,
                },
            ],
            taxes: vec![
                TaxSum {
                    rate: 0.19,
                    basis: 2000,
                    tax: 380,
                },
                TaxSum {
                    rate: 0.0,
                    basis: 2125,
                    tax: 0,
                },
            ],
            net: 4125,
            tax: 380,
            gross: 4505,
            payment: Payment {
                iban: Some("DE02120300000000202051".into()),
                bic: None,
                account_name: None,
                terms: None,
                due: NaiveDate::from_ymd_opt(2026, 2, 3),
            },
            exemption_reason: Some("Kleinunternehmer".into()),
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(amount(4505), "45.05");
        assert_eq!(amount(-5), "-0.05");
        assert_eq!(amount(0), "0.00");
        assert_eq!(decimal(8.0, 4), "8");
        assert_eq!(decimal(2.5, 4), "2.5");
        assert_eq!(percent(0.19), "19");
        assert_eq!(percent(0.07), "7");
        assert_eq!(percent(0.0), "0");
        assert_eq!(category(0.0), "E");
        assert_eq!(category(0.07), "S");
    }

    #[test]
    fn renders_in_schema_order() {
        let xml = render(&sample());
        // escaped
        assert!(xml.contains("<ram:Name>Café &lt;ascii&gt; &amp; Co</ram:Name>"));
        assert!(!xml.contains("<ascii>"));
        // the order the CII schema demands
        let order = [
            "<rsm:ExchangedDocumentContext>",
            PROFILE,
            "<rsm:ExchangedDocument>",
            "<ram:TypeCode>380</ram:TypeCode>",
            "<udt:DateTimeString format=\"102\">20260120</udt:DateTimeString>",
            "<ram:IncludedSupplyChainTradeLineItem>",
            "<ram:BilledQuantity unitCode=\"HUR\">2.5</ram:BilledQuantity>",
            "<ram:ApplicableHeaderTradeAgreement>",
            "<ram:SellerTradeParty>",
            "<ram:ID schemeID=\"VA\">DE123456789</ram:ID>",
            "<ram:BuyerTradeParty>",
            "<ram:URIID schemeID=\"EM\">b@example.com</ram:URIID>",
            "<ram:ApplicableHeaderTradeDelivery>",
            "20260115",
            "<ram:ApplicableHeaderTradeSettlement>",
            "<ram:PaymentReference>R2026-030</ram:PaymentReference>",
            "<ram:InvoiceCurrencyCode>EUR</ram:InvoiceCurrencyCode>",
            "<ram:TypeCode>58</ram:TypeCode>",
            "<ram:CalculatedAmount>3.80</ram:CalculatedAmount>",
            "<ram:ExemptionReason>Kleinunternehmer</ram:ExemptionReason>",
            "<ram:SpecifiedTradePaymentTerms>",
            "20260203",
            "<ram:LineTotalAmount>41.25</ram:LineTotalAmount>",
            "<ram:TaxTotalAmount currencyID=\"EUR\">3.80</ram:TaxTotalAmount>",
            "<ram:GrandTotalAmount>45.05</ram:GrandTotalAmount>",
            "<ram:DuePayableAmount>45.05</ram:DuePayableAmount>",
            "</rsm:CrossIndustryInvoice>",
        ];
        let mut at = 0;
        for needle in order {
            let found = xml[at..].find(needle).unwrap_or_else(|| panic!("{} not after {}\n{}", needle, at, xml));
            at += found + needle.len();
        }
        // only one exemption reason, at the 0 % sum
        assert_eq!(xml.matches("ExemptionReason").count(), 2);
        // balanced tags
        assert_eq!(xml.matches("<ram:").count(), xml.matches("</ram:").count());
    }
}
