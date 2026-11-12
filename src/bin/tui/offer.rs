//! The products of a project (the offer) and its catalog, read from and written back to the project file.
//!
//! A project file keeps its catalog as YAML anchors and refers to them from the product list:
//!
//! ```yaml
//! cataloge:
//!   product: &kaffee { name: Kaffee, price: 2.5, unit: 1l }
//! products:
//!   *kaffee: { amount: 4, sold: 5 }
//!   "Old style": { amount: 1, price: 3, unit: stk }   # without catalog
//! ```
//!
//! The parser resolves the anchors, so their names are read from the text. Only the `cataloge:` and
//! `products:` sections are rewritten, everything else in the file stays as it is. The format is the one
//! the library reads; nothing new is added to it.

use anyhow::{bail, Error};
use yaml_rust::{Yaml, YamlLoader};

use asciii::project::spec::Redeemable;
use asciii::project::Project;

use super::yaml_edit::flow_scalar;

/// One product of the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogItem {
    /// the YAML anchor, e.g. `kaffee` for `&kaffee`
    pub anchor: String,
    pub name: String,
    pub price: f64,
    pub unit: Option<String>,
    pub tax: Option<f64>,
}

impl CatalogItem {
    /// name, unit and price, for lists
    pub fn describe(&self) -> String {
        let unit = self.unit.as_deref().map(|u| format!(" ({})", u)).unwrap_or_default();
        format!("{}{}  {}", self.name, unit, money(self.price))
    }

    fn same_values(&self, other: &CatalogItem) -> bool {
        self.name == other.name && self.price == other.price && self.unit == other.unit && self.tax == other.tax
    }
}

/// One line of the offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// the catalog entry, `None` for the old format with the name as key
    pub anchor: Option<String>,
    /// only used without catalog entry, otherwise the catalog has it
    pub name: String,
    pub price: f64,
    pub unit: Option<String>,
    pub amount: f64,
    pub sold: Option<f64>,
    pub returned: Option<f64>,
    /// other keys of the entry (e.g. `tax`), written back unchanged
    pub extra: Vec<(String, String)>,
}

impl Line {
    /// What was actually used: `sold`, or offered minus `returned`, or the offered amount if nothing is noted.
    pub fn used(&self) -> f64 {
        match (self.sold, self.returned) {
            (Some(sold), _) => sold,
            (None, Some(returned)) => self.amount - returned,
            (None, None) => self.amount,
        }
    }

    /// Whether the actual usage is written down (`sold` or `returned`).
    pub fn counted(&self) -> bool {
        self.sold.is_some() || self.returned.is_some()
    }

    /// Notes the actual usage, the way this line already does it (`returned` stays `returned` if possible).
    pub fn set_used(&mut self, used: f64) {
        if self.returned.is_some() && used <= self.amount {
            self.returned = Some(self.amount - used);
            self.sold = None;
        } else {
            self.sold = Some(used);
            self.returned = None;
        }
    }
}

/// What the offer editor works on.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub catalog: Vec<CatalogItem>,
    pub lines: Vec<Line>,
}

impl Offer {
    pub fn item(&self, anchor: &str) -> Option<&CatalogItem> {
        self.catalog.iter().find(|i| i.anchor == anchor)
    }

    /// Name, unit and price of a line, from the catalog if it has an entry.
    pub fn resolved(&self, line: &Line) -> (String, Option<String>, f64) {
        match line.anchor.as_deref().and_then(|a| self.item(a)) {
            Some(item) => (item.name.clone(), item.unit.clone(), item.price),
            None => (line.name.clone(), line.unit.clone(), line.price),
        }
    }

    /// Net sum of what was used (the invoice).
    pub fn used_net(&self) -> f64 {
        self.lines.iter().map(|l| self.resolved(l).2 * l.used()).sum()
    }

    /// Net sum of the offered amounts.
    pub fn net(&self) -> f64 {
        self.lines.iter().map(|l| self.resolved(l).2 * l.amount).sum()
    }

    /// Changes name, unit and price of a line; for catalog products in the catalog of this project.
    pub fn set_product(&mut self, index: usize, name: String, unit: Option<String>, price: f64) {
        let Some(line) = self.lines.get_mut(index) else { return };
        match line.anchor.clone().and_then(|a| self.catalog.iter_mut().find(|i| i.anchor == a)) {
            Some(item) => {
                item.name = name;
                item.unit = unit;
                item.price = price;
            },
            None => {
                line.name = name;
                line.unit = unit;
                line.price = price;
            },
        }
    }

    /// Adds a catalog product (copying it into this project's catalog if needed) or adds to its amount.
    pub fn add_item(&mut self, item: &CatalogItem, amount: f64) -> usize {
        if let Some(index) = self.lines.iter().position(|l| l.anchor.as_deref() == Some(&item.anchor)) {
            self.lines[index].amount += amount;
            return index;
        }
        if self.item(&item.anchor).is_none() {
            self.catalog.push(item.clone());
        }
        self.lines.push(Line {
            anchor: Some(item.anchor.clone()),
            name: String::new(),
            price: 0.0,
            unit: None,
            amount,
            sold: None,
            returned: None,
            extra: Vec::new(),
        });
        self.lines.len() - 1
    }

    /// A product that is in no catalog yet: gets a catalog entry with a new anchor.
    pub fn add_new(&mut self, name: String, unit: Option<String>, price: f64, amount: f64) -> usize {
        let anchor = self.free_anchor(&name);
        let item = CatalogItem {
            anchor,
            name,
            price,
            unit,
            tax: None,
        };
        self.add_item(&item, amount)
    }

    pub fn free_anchor(&self, name: &str) -> String {
        let base = anchor_for(name);
        let taken = |a: &str| self.catalog.iter().any(|i| i.anchor == a);
        if !taken(&base) {
            return base;
        }
        (2..).map(|n| format!("{}_{}", base, n)).find(|a| !taken(a)).unwrap_or(base)
    }
}

/// A YAML anchor made from a product name: `Kekse Lamberts` → `kekse_lamberts`.
pub fn anchor_for(name: &str) -> String {
    let mut anchor = String::new();
    for c in name.to_lowercase().chars() {
        match c {
            'ä' => anchor.push_str("ae"),
            'ö' => anchor.push_str("oe"),
            'ü' => anchor.push_str("ue"),
            'ß' => anchor.push_str("ss"),
            c if c.is_ascii_alphanumeric() => anchor.push(c),
            _ if !anchor.ends_with('_') => anchor.push('_'),
            _ => {},
        }
    }
    let anchor = anchor.trim_matches('_').to_owned();
    if anchor.is_empty() {
        String::from("product")
    } else {
        anchor
    }
}

/// `2,50€`
pub fn money(value: f64) -> String {
    format!("{:.2}€", value).replace('.', ",")
}

/// A number as short as possible: `4`, `2.5`, `0.61`.
pub fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{:.4}", value);
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Reads `2,50`, `2.5` or `2,50€`.
pub fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim().trim_end_matches('€').trim().replace(',', ".");
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

// ── sections ───────────────────────────────────────────────────────────────

/// Lines `start..end` of a top-level section; `end` excludes blank lines and comments in front of the
/// next section.
fn section(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let is_top_level = |line: &str| {
        let line = line.trim_end();
        !line.is_empty() && !line.starts_with([' ', '\t', '#']) && (line.contains(':') || line == "..." || line == "---")
    };
    let header = format!("{}:", key);
    let start = lines.iter().position(|l| l.trim_end() == header || l.starts_with(&format!("{} ", header)))?;
    let mut end = (start + 1..lines.len()).find(|&n| is_top_level(lines[n])).unwrap_or(lines.len());
    while end > start + 1 {
        let line = lines[end - 1].trim_end();
        if line.is_empty() || line.starts_with('#') {
            end -= 1;
        } else {
            break;
        }
    }
    Some((start, end))
}

fn get_f64(yaml: &Yaml, key: &str) -> Option<f64> {
    match &yaml[key] {
        Yaml::Real(r) => r.parse().ok(),
        Yaml::Integer(i) => Some(*i as f64),
        Yaml::String(s) => parse_number(s),
        _ => None,
    }
}

fn get_string(yaml: &Yaml, key: &str) -> Option<String> {
    match &yaml[key] {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        _ => None,
    }
}

fn scalar_text(yaml: &Yaml) -> Option<String> {
    match yaml {
        Yaml::String(s) => Some(flow_scalar(s)),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The catalog of a project or template file.
pub fn read_catalog(content: &str) -> Result<Vec<CatalogItem>, Error> {
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = section(&lines, "cataloge") else {
        return Ok(Vec::new());
    };
    let entry = regex::Regex::new(r"^(\s+)[\w-]+\s*:\s*&([\w-]+)\s*(.*)$").expect("valid regex");
    let mut items = Vec::new();
    let mut n = start + 1;
    while n < end {
        let Some(caps) = entry.captures(lines[n]) else {
            n += 1;
            continue;
        };
        let indent = caps[1].len();
        let anchor = caps[2].to_owned();
        let mut text = caps[3].to_owned();
        n += 1;
        // block style: the values are on the following, deeper indented lines
        if text.trim().is_empty() || text.trim_start().starts_with('#') {
            let mut block = Vec::new();
            while n < end && (lines[n].trim().is_empty() || lines[n].len() - lines[n].trim_start().len() > indent) {
                block.push(lines[n].trim());
                n += 1;
            }
            text = block.join("\n");
        }
        let desc = YamlLoader::load_from_str(&text)
            .ok()
            .and_then(|docs| docs.into_iter().next())
            .unwrap_or(Yaml::Null);
        let Some(price) = get_f64(&desc, "price") else {
            bail!(lformat!("catalog entry {} has no valid price, fix it with the editor (e)", anchor));
        };
        items.push(CatalogItem {
            name: get_string(&desc, "name").unwrap_or_else(|| anchor.clone()),
            anchor,
            price,
            unit: get_string(&desc, "unit"),
            tax: get_f64(&desc, "tax"),
        });
    }
    Ok(items)
}

/// Catalog and products of a project file.
pub fn read(content: &str) -> Result<Offer, Error> {
    let catalog = read_catalog(content)?;
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = section(&lines, "products") else {
        bail!(lformat!("the project file has no products section, add one with the editor (e)"));
    };
    // `*kaffee:` keys would be resolved to the catalog entries, make them plain strings to keep the name
    let alias = regex::Regex::new(r"^(\s+)\*([\w-]+)\s*:").expect("valid regex");
    let text: Vec<String> = lines[start..end].iter().map(|l| alias.replace(l, "$1\"*$2\":").into_owned()).collect();
    let doc = YamlLoader::load_from_str(&text.join("\n"))
        .map_err(|e| anyhow::format_err!("{}", lformat!("the products section is not valid YAML: {}", e)))?
        .into_iter()
        .next()
        .unwrap_or(Yaml::Null);

    let mut offer_lines = Vec::new();
    if let Some(products) = doc["products"].as_hash() {
        for (key, values) in products {
            let Some(key) = key.as_str() else {
                bail!(lformat!("a product has an unexpected name, fix it with the editor (e)"));
            };
            let anchor = key.strip_prefix('*').map(ToOwned::to_owned);
            if let Some(anchor) = &anchor {
                if !catalog.iter().any(|i| &i.anchor == anchor) {
                    bail!(lformat!("product *{} is not in the catalog, fix it with the editor (e)", anchor));
                }
            }
            let managed: &[&str] = if anchor.is_some() {
                &["amount", "sold", "returned"]
            } else {
                &["amount", "sold", "returned", "price", "unit"]
            };
            let extra = values
                .as_hash()
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| Some((k.as_str()?.to_owned(), scalar_text(v)?)))
                        .filter(|(k, _)| !managed.contains(&k.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            offer_lines.push(Line {
                name: if anchor.is_some() { String::new() } else { key.to_owned() },
                anchor,
                price: get_f64(values, "price").unwrap_or(0.0),
                unit: get_string(values, "unit"),
                amount: get_f64(values, "amount").unwrap_or(0.0),
                sold: get_f64(values, "sold"),
                returned: get_f64(values, "returned"),
                extra,
            });
        }
    }
    Ok(Offer {
        catalog,
        lines: offer_lines,
    })
}

// ── writing ────────────────────────────────────────────────────────────────
//
// Only what changed is written: unchanged entries keep their lines byte for byte, changed values are
// replaced in place (keeping the alignment), new entries look like their neighbours. That keeps the
// git history of projects and templates readable.

/// One entry of a section and its lines `start..end` (with the blank lines and comments after it).
struct SectionEntry {
    key: String,
    start: usize,
    end: usize,
}

/// The entries of the section `start..end`: the lines at the indentation of the first one.
fn section_entries(lines: &[&str], start: usize, end: usize, key_of: impl Fn(&str) -> Option<String>) -> Vec<SectionEntry> {
    let mut indent = None;
    let mut found: Vec<SectionEntry> = Vec::new();
    for (n, line) in lines.iter().enumerate().take(end).skip(start + 1) {
        let trimmed = line.trim_start();
        if trimmed.trim().is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let this = line.len() - trimmed.len();
        if this != *indent.get_or_insert(this) {
            continue;
        }
        if let Some(key) = key_of(line) {
            if let Some(last) = found.last_mut() {
                last.end = n;
            }
            found.push(SectionEntry { key, start: n, end });
        }
    }
    found
}

fn catalog_key(line: &str) -> Option<String> {
    let entry = regex::Regex::new(r"^\s+[\w-]+\s*:\s*&([\w-]+)").expect("valid regex");
    entry.captures(line).map(|c| c[1].to_owned())
}

fn product_key(line: &str) -> Option<String> {
    let alias = regex::Regex::new(r"^\s+\*([\w-]+)\s*:").expect("valid regex");
    if let Some(caps) = alias.captures(line) {
        return Some(format!("*{}", &caps[1]));
    }
    let doc = YamlLoader::load_from_str(line.trim()).ok()?.into_iter().next()?;
    doc.as_hash()?.keys().next()?.as_str().map(ToOwned::to_owned)
}

fn product_key_of(line: &Line) -> String {
    match &line.anchor {
        Some(anchor) => format!("*{}", anchor),
        None => line.name.clone(),
    }
}

/// `value` written like `old`: with as many decimals, if that is exact (`3.40` → `3.60`).
fn like(old: &str, value: &str) -> String {
    let (Ok(_), Ok(number)) = (old.parse::<f64>(), value.parse::<f64>()) else {
        return value.to_owned();
    };
    let decimals = old.split_once('.').map_or(0, |(_, d)| d.len());
    let formatted = format!("{:.*}", decimals, number);
    if formatted.parse::<f64>().ok() == Some(number) {
        formatted
    } else {
        value.to_owned()
    }
}

/// Changes values inside a one-line `{ key: value, … }`, keeping the columns where possible.
/// `fields` are (key, new value); `None` if the line has to be written new (a field is missing or extra).
fn edit_flow_line(line: &str, fields: &[(&str, Option<String>)]) -> Option<String> {
    let open = line.find('{')?;
    let close = line.rfind('}')?;
    let inner = &line[open + 1..close];
    if inner.contains(['"', '\'', '{', '[']) {
        return None;
    }
    let mut segments: Vec<String> = inner.split(',').map(ToOwned::to_owned).collect();
    let keys: Vec<String> = segments
        .iter()
        .map(|s| s.split_once(':').map(|(k, _)| k.trim().to_owned()).unwrap_or_default())
        .collect();
    // the line must have exactly these fields, otherwise it is written new
    let mut wanted: Vec<&str> = fields.iter().filter(|(_, v)| v.is_some()).map(|(k, _)| *k).collect();
    let mut present: Vec<&str> = keys.iter().map(String::as_str).filter(|k| fields.iter().any(|(f, _)| f == k)).collect();
    wanted.sort_unstable();
    present.sort_unstable();
    if wanted != present {
        return None;
    }
    for (key, value) in fields {
        let Some(value) = value else { continue };
        let index = keys.iter().position(|k| k == key)?;
        let segment = &segments[index];
        let (before, old) = segment.split_once(':')?;
        let old_value = old.trim();
        let value = like(old_value, value);
        if value == old_value {
            continue;
        }
        // `: ` and the old padding: keep the segment as wide as before, but at least one space before `,`
        let lead = old.len() - old.trim_start().len();
        let trail = old.len() - old.trim_end().len();
        let width = old_value.chars().count() + trail;
        let pad = width.saturating_sub(value.chars().count()).max(usize::from(trail > 0));
        segments[index] = format!("{}:{}{}{}", before, " ".repeat(lead), value, " ".repeat(pad));
    }
    Some(format!("{}{{{}}}{}", &line[..open], segments.join(","), &line[close + 1..]))
}

/// A block entry with `fields`: values changed in place, keys without value removed, new keys added at the
/// end with the indentation of the others. `None` if the block has a line it does not understand.
fn edit_block(entry: &[&str], fields: &[(&str, Option<String>)]) -> Option<String> {
    let is_filler = |l: &str| l.trim().is_empty() || l.trim_start().starts_with('#');
    let head = entry.first()?;
    if !head.trim_end().ends_with(':') {
        return None;
    }
    // blank lines and comments at the end stay where they are (see `rebuild`)
    let mut last = entry.len();
    while last > 1 && is_filler(entry[last - 1]) {
        last -= 1;
    }
    let key_value = regex::Regex::new(r"^(\s+)([\w-]+)(\s*:\s*)(.*?)(\s+#.*)?$").expect("valid regex");
    let mut out = vec![(*head).to_owned()];
    let mut seen: Vec<String> = Vec::new();
    let mut indent: Option<String> = None;
    for line in &entry[1..last] {
        if is_filler(line) {
            out.push((*line).to_owned());
            continue;
        }
        let caps = key_value.captures(line)?;
        let key = caps[2].to_owned();
        let (_, value) = fields.iter().find(|(k, _)| *k == key)?;
        indent.get_or_insert_with(|| caps[1].to_owned());
        if let Some(value) = value {
            let old = &caps[4];
            let comment = caps.get(5).map_or("", |m| m.as_str());
            out.push(format!("{}{}{}{}{}", &caps[1], key, &caps[3], like(old, value), comment));
        }
        seen.push(key);
    }
    let indent = indent?;
    for (key, value) in fields {
        if let (Some(value), false) = (value, seen.iter().any(|s| s == key)) {
            out.push(format!("{}{}: {}", indent, key, value));
        }
    }
    Some(format!("{}\n", out.join("\n")))
}

fn catalog_fields(item: &CatalogItem)
 -> [(&'static str, Option<String>); 4] {
    [
        ("name", Some(flow_scalar(&item.name))),
        ("price", Some(number(item.price))),
        ("unit", item.unit.as_deref().map(flow_scalar)),
        ("tax", item.tax.map(number)),
    ]
}

/// A catalog line for `item`: like `model` (a line of the same catalog) if possible.
fn catalog_line(item: &CatalogItem, model: Option<&str>) -> String {
    let from_model = model.and_then(|model| {
        // the anchor where the model has its own, padded to the same column
        let caps = regex::Regex::new(r"&([\w-]+)(\s*)").expect("valid regex").captures(model)?;
        let whole = caps.get(0)?;
        let width = whole.as_str().chars().count();
        let anchor = format!("&{}", item.anchor);
        let pad = width.saturating_sub(anchor.chars().count()).max(1);
        let line = format!("{}{}{}{}", &model[..whole.start()], anchor, " ".repeat(pad), &model[whole.end()..]);
        let line = edit_flow_line(line.trim_end_matches(['\n', '\r']), &catalog_fields(item))?;
        // a value the model could not take (e.g. quoting) makes it a plain new line
        (read_catalog(&format!("cataloge:\n{}\n", line)).ok()? == [item.clone()]).then_some(line)
    });
    let line = from_model.unwrap_or_else(|| {
        let fields: Vec<String> = catalog_fields(item)
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| format!("{}: {}", k, v)))
            .collect();
        format!("  product: &{} {{ {} }}", item.anchor, fields.join(", "))
    });
    format!("{}\n", line)
}

fn product_fields(line: &Line) -> Vec<(&str, Option<String>)> {
    let mut fields = vec![
        ("amount", Some(number(line.amount))),
        ("sold", line.sold.map(number)),
        ("returned", line.returned.map(number)),
    ];
    if line.anchor.is_none() {
        fields.push(("price", Some(number(line.price))));
        fields.push(("unit", line.unit.as_deref().map(flow_scalar)));
    }
    fields
}

/// A product line for `line`, `indent` deep.
fn product_line(line: &Line, indent: usize) -> String {
    let key = match &line.anchor {
        Some(anchor) => format!("*{}", anchor),
        None => format!("\"{}\"", line.name.replace('\\', "\\\\").replace('"', "\\\"")),
    };
    let mut fields: Vec<String> = product_fields(line)
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| format!("{}: {}", k, v)))
        .collect();
    fields.extend(line.extra.iter().map(|(k, v)| format!("{}: {}", k, v)));
    format!("{}{}: {{ {} }}\n", " ".repeat(indent), key, fields.join(", "))
}

/// What happens to an entry of a section.
enum Wanted {
    /// unchanged, the old lines stay
    Keep,
    /// these lines instead
    Text(String),
}

/// The section `start..end` with entries kept, replaced, removed (not in `wanted`) or added (at the end).
fn rebuild(lines: &[&str], start: usize, end: usize, found: &[SectionEntry], wanted: &[(String, Wanted)]) -> String {
    let is_filler = |l: &str| l.trim().is_empty() || l.trim_start().starts_with('#');
    let mut out: String = lines[start..found.first().map_or(end, |e| e.start)].concat();
    let ensure_newline = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    };
    for entry in found {
        match wanted.iter().find(|(key, _)| *key == entry.key).map(|(_, w)| w) {
            Some(Wanted::Keep) => out.push_str(&lines[entry.start..entry.end].concat()),
            Some(Wanted::Text(text)) => {
                // blank lines and comments after the entry stay
                let mut tail = entry.end;
                while tail > entry.start + 1 && is_filler(lines[tail - 1]) {
                    tail -= 1;
                }
                ensure_newline(&mut out);
                out.push_str(text);
                out.push_str(&lines[tail..entry.end].concat());
            },
            None => {},
        }
    }
    for (key, wanted) in wanted {
        if let (false, Wanted::Text(text)) = (found.iter().any(|e| e.key == *key), wanted) {
            ensure_newline(&mut out);
            out.push_str(text);
        }
    }
    out
}

/// Writes the catalog `items` into `content`, touching only what changed.
fn write_catalog(content: &str, items: &[CatalogItem]) -> Result<String, Error> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end_matches(['\n', '\r'])).collect();
    let old = read_catalog(content)?;
    let Some((start, end)) = section(&trimmed, "cataloge") else {
        if items.is_empty() {
            return Ok(content.to_owned());
        }
        // no catalog yet: a new section in front of the products, the aliases refer back to it
        let Some((at, _)) = section(&trimmed, "products") else {
            bail!(lformat!("the project file has no products section, add one with the editor (e)"));
        };
        let mut text = String::from("cataloge:\n");
        items.iter().for_each(|item| text.push_str(&catalog_line(item, None)));
        return Ok(format!("{}{}\n{}", lines[..at].concat(), text, lines[at..].concat()));
    };
    let found = section_entries(&trimmed, start, end, catalog_key);
    // the last one-line entry is the model for new lines
    let model = found
        .iter()
        .rev()
        .find(|e| trimmed[e.start].contains('{') && !trimmed[e.start].contains(['"', '\''] ))
        .map(|e| trimmed[e.start]);
    let wanted: Vec<(String, Wanted)> = items
        .iter()
        .map(|item| {
            let entry = found.iter().find(|e| e.key == item.anchor);
            let wanted = match (old.iter().find(|o| o.anchor == item.anchor), entry) {
                (Some(o), Some(_)) if o == item => Wanted::Keep,
                // changed: in place if it is a one-line entry
                (_, Some(entry)) if entry.end - entry.start >= 1 && trimmed[entry.start].contains('{') => {
                    match edit_flow_line(trimmed[entry.start], &catalog_fields(item)) {
                        Some(line) if read_catalog(&format!("cataloge:\n{}\n", line)).ok().as_deref() == Some(std::slice::from_ref(item)) => {
                            Wanted::Text(format!("{}\n", line))
                        },
                        _ => Wanted::Text(catalog_line(item, model)),
                    }
                },
                _ => Wanted::Text(catalog_line(item, model)),
            };
            (item.anchor.clone(), wanted)
        })
        .collect();
    let section_text = rebuild(&lines, start, end, &found, &wanted);
    Ok(format!("{}{}{}", lines[..start].concat(), section_text, lines[end..].concat()))
}

/// Writes the product `lines` into `content`, touching only what changed.
fn write_products(content: &str, offer_lines: &[Line]) -> Result<String, Error> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let trimmed: Vec<&str> = lines.iter().map(|l| l.trim_end_matches(['\n', '\r'])).collect();
    let Some((start, end)) = section(&trimmed, "products") else {
        bail!(lformat!("the project file has no products section, add one with the editor (e)"));
    };
    let old = read(content)?.lines;
    let found = section_entries(&trimmed, start, end, product_key);
    let indent = found
        .first()
        .map_or(2, |e| trimmed[e.start].len() - trimmed[e.start].trim_start().len());
    let wanted: Vec<(String, Wanted)> = offer_lines
        .iter()
        .map(|line| {
            let key = product_key_of(line);
            let entry = found.iter().find(|e| e.key == key);
            let wanted = match (old.iter().find(|o| product_key_of(o) == key), entry) {
                (Some(o), Some(_)) if o == line => Wanted::Keep,
                (_, Some(entry)) if entry.end > entry.start && trimmed[entry.start].contains('{') => {
                    // the other keys (e.g. `tax`) must stay as they are
                    let mut fields = product_fields(line);
                    fields.extend(line.extra.iter().map(|(k, v)| (k.as_str(), Some(v.clone()))));
                    match edit_flow_line(trimmed[entry.start], &fields) {
                        Some(text) if entry.end == entry.start + 1 || trimmed[entry.start + 1..entry.end].iter().all(|l| l.trim().is_empty() || l.trim_start().starts_with('#')) => {
                            Wanted::Text(format!("{}\n", text))
                        },
                        _ => Wanted::Text(product_line(line, indent)),
                    }
                },
                // a block entry (`*kaffee:` with one key per line) keeps its form: only changed keys change
                (_, Some(entry)) if entry.end > entry.start + 1 => {
                    let mut fields = product_fields(line);
                    fields.extend(line.extra.iter().map(|(k, v)| (k.as_str(), Some(v.clone()))));
                    match edit_block(&trimmed[entry.start..entry.end], &fields) {
                        Some(text) => Wanted::Text(text),
                        None => Wanted::Text(product_line(line, indent)),
                    }
                },
                _ => Wanted::Text(product_line(line, indent)),
            };
            (key, wanted)
        })
        .collect();
    let section_text = rebuild(&lines, start, end, &found, &wanted);
    Ok(format!("{}{}{}", lines[..start].concat(), section_text, lines[end..].concat()))
}

/// The project file with catalog and products of `offer`.
pub fn write(content: &str, offer: &Offer) -> Result<String, Error> {
    let with_products = write_products(content, &offer.lines)?;
    let result = write_catalog(&with_products, &offer.catalog)?;
    check_roundtrip(content, &result, offer)?;
    Ok(result)
}

/// The template with `items` added to or changed in its catalog.
pub fn update_template(content: &str, items: &[CatalogItem]) -> Result<String, Error> {
    let mut catalog = read_catalog(content)?;
    for item in items {
        match catalog.iter_mut().find(|i| i.anchor == item.anchor) {
            Some(existing) => *existing = item.clone(),
            None => catalog.push(item.clone()),
        }
    }
    let result = write_catalog(content, &catalog)?;
    if read_catalog(&result)? != catalog || YamlLoader::load_from_str(&result).is_err() {
        bail!(lformat!("the template catalog could not be written safely, nothing changed"));
    }
    Ok(result)
}

/// The written file must parse and give back exactly what was written.
fn check_roundtrip(original: &str, content: &str, offer: &Offer) -> Result<(), Error> {
    if let Err(error) = YamlLoader::load_from_str(content) {
        bail!(lformat!("the offer would break the project file, nothing saved: {}", error));
    }
    if read(content)? != *offer {
        bail!(lformat!("the offer could not be written safely, nothing saved"));
    }
    // and the library must compute the same offer from it. Its bill also has items that are not
    // products (e.g. the service hours), those must stay the same.
    // (items, sum before tax) of offer and invoice; the library leaves out products of quantity 0
    let library = |content: &str| -> Result<[(usize, f64); 2], Error> {
        let project = Project::from_file_content(content)?;
        let (offer_bill, invoice_bill) = project.bills()?;
        // the bill crate calls the sum before tax "gross"
        Ok([
            (offer_bill.as_items().iter().filter(|i| i.amount != 0.0).count(), offer_bill.gross_total().as_float()),
            (invoice_bill.as_items().iter().filter(|i| i.amount != 0.0).count(), invoice_bill.gross_total().as_float()),
        ])
    };
    let model = |o: &Offer| -> [(usize, f64); 2] {
        [
            (o.lines.iter().filter(|l| l.amount != 0.0).count(), o.net()),
            (o.lines.iter().filter(|l| l.used() != 0.0).count(), o.used_net()),
        ]
    };
    // what the library has besides the products (e.g. the service hours) must stay the same
    let others = match (read(original), library(original)) {
        (Ok(before), Ok(lib)) => {
            let before = model(&before);
            [0, 1].map(|b| (lib[b].0.saturating_sub(before[b].0), lib[b].1 - before[b].1))
        },
        // nothing to compare with (no products yet), so nothing else
        _ => [(0, 0.0); 2],
    };
    let now = library(content)?;
    let expected = model(offer);
    for b in [0, 1] {
        if now[b].0 != expected[b].0 + others[b].0 || (now[b].1 - expected[b].1 - others[b].1).abs() > 0.005 {
            bail!(lformat!(
                "the offer would read back differently ({} items, {}), nothing saved",
                now[b].0,
                money(now[b].1)
            ));
        }
    }
    Ok(())
}

// ── syncing back to the template ───────────────────────────────────────────

/// The template a project was made from (`meta/template`, otherwise the configured default) and its catalog.
pub fn template_catalog(project_content: &str) -> Option<(std::path::PathBuf, Vec<CatalogItem>)> {
    let name = super::yaml_edit::value_at(project_content, "meta/template")
        .or_else(|| asciii::CONFIG.get_str_or("template").map(ToOwned::to_owned))
        .unwrap_or_else(|| String::from("default"));
    let storage = asciii::storage::setup::<Project>().ok()?;
    let file = storage.get_template_file(&name).ok()?;
    let catalog = read_catalog(&std::fs::read_to_string(&file).ok()?).ok()?;
    Some((file, catalog))
}

/// A catalog entry of the project that differs from the template.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub item: CatalogItem,
    /// the template's version, `None` for a new product
    pub template: Option<CatalogItem>,
}

impl Change {
    pub fn describe(&self) -> String {
        match &self.template {
            None => lformat!("new: {}", self.item.describe()),
            Some(old) => {
                let mut parts = Vec::new();
                if old.name != self.item.name {
                    parts.push(format!("{} → {}", old.name, self.item.name));
                }
                if old.unit != self.item.unit {
                    parts.push(lformat!(
                        "unit {} → {}",
                        old.unit.as_deref().unwrap_or("–"),
                        self.item.unit.as_deref().unwrap_or("–")
                    ));
                }
                if old.price != self.item.price {
                    parts.push(format!("{} → {}", money(old.price), money(self.item.price)));
                }
                if old.tax != self.item.tax {
                    parts.push(lformat!(
                        "tax {} → {}",
                        old.tax.map(number).unwrap_or_else(|| String::from("–")),
                        self.item.tax.map(number).unwrap_or_else(|| String::from("–"))
                    ));
                }
                format!("{}: {}", self.item.name, parts.join(", "))
            },
        }
    }
}

/// Catalog entries changed from `before` to `after` that are not like that in the template yet.
pub fn changes(before: &[CatalogItem], after: &[CatalogItem], template: &[CatalogItem]) -> Vec<Change> {
    after
        .iter()
        .filter(|item| !before.iter().any(|b| b.anchor == item.anchor && b.same_values(item)))
        .filter_map(|item| {
            let in_template = template.iter().find(|t| t.anchor == item.anchor);
            match in_template {
                Some(t) if t.same_values(item) => None,
                _ => Some(Change {
                    item: item.clone(),
                    template: in_template.cloned(),
                }),
            }
        })
        .collect()
}

// ── templates ──────────────────────────────────────────────────────────────
//
// A template (`templates/*.tyml`) has the same `cataloge:` and `products:` sections as a project: the
// price list and the offer every new project starts with. It also has `##PLACEHOLDER##` marks that
// are filled when a project is created; they stay exactly as they are.

/// Catalog and starting offer of a template. A template without `products:` has no starting offer.
pub fn read_template(content: &str) -> Result<Offer, Error> {
    let lines: Vec<&str> = content.lines().collect();
    if section(&lines, "products").is_none() {
        return Ok(Offer {
            catalog: read_catalog(content)?,
            lines: Vec::new(),
        });
    }
    read(content)
}

/// Whether the template has a `products:` section (needed for a starting offer).
pub fn has_products(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    section(&lines, "products").is_some()
}

/// Whether the template has a `cataloge:` section.
pub fn has_catalog(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    section(&lines, "cataloge").is_some()
}

/// The template with catalog and starting offer of `wanted`, touching only the changed lines, and
/// checked: it reads back the same, everything outside the two sections and every placeholder is
/// unchanged, and a project created from it opens and has this offer.
pub fn write_template(original: &str, wanted: &Offer) -> Result<String, Error> {
    let with_products = if has_products(original) {
        write_products(original, &wanted.lines)?
    } else if wanted.lines.is_empty() {
        original.to_owned()
    } else {
        bail!(lformat!("the template has no products section for a starting offer; add \"products:\" in the editor"));
    };
    let result = write_catalog(&with_products, &wanted.catalog)?;
    check_template(original, &result, wanted)?;
    Ok(result)
}

/// All `##NAME##` marks, in order.
fn placeholders(content: &str) -> Vec<String> {
    let mark = regex::Regex::new(r"##[A-Za-z0-9_-]+##").expect("valid regex");
    mark.find_iter(content).map(|m| m.as_str().to_owned()).collect()
}

/// The lines outside `cataloge:` and `products:`.
fn outside_sections(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut skip = Vec::new();
    for key in ["cataloge", "products"] {
        if let Some((start, end)) = section(&lines, key) {
            skip.push((start, end));
        }
    }
    lines
        .iter()
        .enumerate()
        .filter(|(n, _)| !skip.iter().any(|(s, e)| n >= s && n < e))
        .map(|(_, l)| (*l).to_owned())
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// What `asciii new` makes of a template: the placeholders filled like the library fills them.
pub fn filled_like_new_project(template: &str) -> String {
    let today = chrono::Utc::today();
    let fill: std::collections::HashMap<&str, String> = [
        ("TEMPLATE", String::from("template")),
        ("PROJECT-NAME", String::from("Test")),
        ("DATE-EVENT", (today + chrono::Duration::days(14)).format("%d.%m.%Y").to_string()),
        ("DATE-CREATED", today.format("%d.%m.%Y").to_string()),
        ("TAX", asciii::CONFIG.get_to_string("defaults/tax")),
        ("SALARY", asciii::CONFIG.get_to_string("defaults/salary")),
        ("MANAGER", String::new()),
        ("TIME-START", String::new()),
        ("TIME-END", String::new()),
        ("VERSION", asciii::VERSION.to_string()),
    ]
    .into_iter()
    .collect();
    asciii::templater::Templater::new(template).fill_in_data(&fill).finalize().filled
}

fn check_template(original: &str, result: &str, wanted: &Offer) -> Result<(), Error> {
    let unsafe_write = || lformat!("the template could not be written safely, nothing saved");
    if read_template(result)? != *wanted {
        bail!(unsafe_write());
    }
    if YamlLoader::load_from_str(original).is_ok() && YamlLoader::load_from_str(result).is_err() {
        bail!(lformat!("the change would break the template file, nothing saved"));
    }
    if placeholders(original) != placeholders(result) || outside_sections(original) != outside_sections(result) {
        bail!(unsafe_write());
    }
    // a new project from it: valid YAML, the library computes its bills, the same products
    let project = filled_like_new_project(result);
    if let Err(error) = YamlLoader::load_from_str(&project) {
        bail!(lformat!("new projects could not be read with this template ({}), nothing saved", error));
    }
    // (the bills need values from the configuration, e.g. the salary: only what worked before has to work)
    let bills_work = |content: &str| {
        YamlLoader::load_from_str(content).is_ok() && Project::from_file_content(content).is_ok_and(|p| p.bills().is_ok())
    };
    if let (true, Err(error)) = (bills_work(&filled_like_new_project(original)), Project::from_file_content(&project)?.bills()) {
        bail!(lformat!("new projects could not be read with this template ({}), nothing saved", error));
    }
    if read_template(&project)? != *wanted {
        bail!(unsafe_write());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
--- # asciii document
client:
  title: Frau

cataloge:
  product: &kaffee       { name: Kaffee          , price: 2.5  , unit: 1l  }
  product: &kekse        { name: Kekse Lamberts  , price: 3.40 , unit: 400g}
  product: &obst
    name: Obstplatte
    price: 30

# what was sold
products:
  *kaffee:
    amount: 4
    sold:   5

  *kekse: { amount: 4, returned: 1, tax: 0.07 }
  \"Sonderwunsch\": { amount: 1, price: 12.5, unit: stk }

hours:
  salary: 8.5
...
";

    #[test]
    fn reads_catalog_and_products() {
        let offer = read(FILE).unwrap();
        assert_eq!(offer.catalog.len(), 3);
        assert_eq!(offer.catalog[1].name, "Kekse Lamberts");
        assert_eq!(offer.catalog[1].price, 3.4);
        assert_eq!(offer.catalog[2].anchor, "obst");
        assert_eq!(offer.catalog[2].unit, None);
        assert_eq!(offer.lines.len(), 3);
        assert_eq!(offer.lines[0].sold, Some(5.0));
        assert_eq!(offer.lines[1].extra, vec![(String::from("tax"), String::from("0.07"))]);
        assert_eq!(offer.lines[2].anchor, None);
        assert_eq!(offer.resolved(&offer.lines[2]), (String::from("Sonderwunsch"), Some(String::from("stk")), 12.5));
        assert_eq!(offer.net(), 4.0 * 2.5 + 4.0 * 3.4 + 12.5);
    }

    #[test]
    fn writes_only_the_two_sections() {
        let mut offer = read(FILE).unwrap();
        offer.lines[0].amount = 6.0;
        let obst = offer.catalog[2].clone();
        offer.add_item(&obst, 2.0);
        offer.add_new(String::from("Club-Mate: kalt, \"lecker\""), Some(String::from("0.5l")), 0.6, 20.0);
        let written = write(FILE, &offer).unwrap();
        assert_eq!(read(&written).unwrap(), offer);
        assert!(written.starts_with("--- # asciii document\nclient:\n  title: Frau\n\ncataloge:\n"));
        // a block entry stays a block, only the changed value changes
        assert!(written.contains("\n# what was sold\nproducts:\n  *kaffee:\n    amount: 6\n    sold:   5\n"));
        assert!(written.ends_with("\nhours:\n  salary: 8.5\n...\n"));
        // the whole file is still what the library expects
        assert!(YamlLoader::load_from_str(&written).unwrap()[0]["products"].as_hash().unwrap().len() == 5);
    }

    #[test]
    fn product_changes_go_to_the_catalog() {
        let mut offer = read(FILE).unwrap();
        offer.set_product(0, String::from("Kaffee"), Some(String::from("1l")), 2.8);
        assert_eq!(offer.item("kaffee").unwrap().price, 2.8);
        offer.set_product(2, String::from("Sonderwunsch"), None, 15.0);
        assert_eq!(offer.lines[2].price, 15.0);
    }

    #[test]
    fn usage_keeps_the_style_of_the_line() {
        let mut offer = read(FILE).unwrap();
        // kaffee: amount 4, sold 5
        assert_eq!(offer.lines[0].used(), 5.0);
        // kekse: amount 4, returned 1
        assert_eq!(offer.lines[1].used(), 3.0);
        offer.lines[1].set_used(2.0);
        assert_eq!((offer.lines[1].sold, offer.lines[1].returned), (None, Some(2.0)));
        // more than offered cannot be written as returned
        offer.lines[1].set_used(6.0);
        assert_eq!((offer.lines[1].sold, offer.lines[1].returned), (Some(6.0), None));
        assert!(!offer.lines[2].counted());
        offer.lines[2].set_used(1.0);
        assert!(offer.lines[2].counted());
        let written = write(FILE, &offer).unwrap();
        assert!(written.contains("  *kekse: { amount: 4, sold: 6, tax: 0.07 }\n"));
        assert!(written.contains("  \"Sonderwunsch\": { amount: 1, sold: 1, price: 12.5, unit: stk }\n"));
        assert_eq!(offer.used_net(), 5.0 * 2.5 + 6.0 * 3.4 + 12.5);
        // used, but not offered: quantity 0 in the offer
        let index = offer.add_new(String::from("Sekt"), Some(String::from("0,75l")), 5.0, 0.0);
        offer.lines[index].set_used(2.0);
        let written = write(FILE, &offer).unwrap();
        assert!(written.contains("  *sekt: { amount: 0, sold: 2 }\n"));
    }

    #[test]
    fn anchors_are_unique_and_plain() {
        assert_eq!(anchor_for("Kekse Lamberts (400g)"), "kekse_lamberts_400g");
        assert_eq!(anchor_for("Brötchen"), "broetchen");
        let mut offer = read(FILE).unwrap();
        let index = offer.add_new(String::from("Kaffee"), None, 3.0, 1.0);
        assert_eq!(offer.lines[index].anchor.as_deref(), Some("kaffee_2"));
    }

    #[test]
    fn needs_a_products_section_but_adds_a_catalog() {
        let file = FILE.replace("# what was sold\nproducts:\n  *kaffee:\n    amount: 4\n    sold:   5\n\n  *kekse: { amount: 4, returned: 1, tax: 0.07 }\n  \"Sonderwunsch\": { amount: 1, price: 12.5, unit: stk }\n\n", "");
        assert!(read(&file).is_err());

        // an old project without catalog gets one in front of the products
        let old = "products:\n  \"Kaffee\": { amount: 2, price: 2.5 }\n\nhours:\n  salary: 8\n";
        let mut offer = read(old).unwrap();
        offer.add_new(String::from("Tee"), Some(String::from("1l")), 1.75, 3.0);
        let written = write(old, &offer).unwrap();
        assert!(written.starts_with("cataloge:\n  product: &tee { name: Tee, price: 1.75, unit: 1l }\n\nproducts:\n  \"Kaffee\": { amount: 2, price: 2.5 }\n  *tee: { amount: 3 }\n"));
    }

    #[test]
    fn changes_are_written_in_place() {
        let mut offer = read(FILE).unwrap();
        // a new price keeps the column and the decimals
        offer.set_product(1, String::from("Kekse Lamberts"), Some(String::from("400g")), 3.6);
        offer.lines[1].amount = 10.0;
        offer.add_new(String::from("Club-Mate"), Some(String::from("0.5l")), 0.6, 20.0);
        let written = write(FILE, &offer).unwrap();
        // everything else stays as it was: one line changed, one added in each section
        let removed: Vec<&str> = FILE.lines().filter(|l| !written.lines().any(|w| w == *l)).collect();
        assert_eq!(removed, vec!["  product: &kekse        { name: Kekse Lamberts  , price: 3.40 , unit: 400g}", "  *kekse: { amount: 4, returned: 1, tax: 0.07 }"]);
        assert_eq!(written.lines().count(), FILE.lines().count() + 2);
        assert!(written.contains("  product: &kekse        { name: Kekse Lamberts  , price: 3.60 , unit: 400g}\n  product: &obst\n    name: Obstplatte\n    price: 30\n  product: &club_mate    { name: Club-Mate       , price: 0.60 , unit: 0.5l}\n"));
        assert!(written.contains("  *kaffee:\n    amount: 4\n    sold:   5\n\n  *kekse: { amount: 10, returned: 1, tax: 0.07 }\n  \"Sonderwunsch\": { amount: 1, price: 12.5, unit: stk }\n  *club_mate: { amount: 20 }\n\nhours:"));
    }

    #[test]
    fn finds_changes_for_the_template() {
        let template = read_catalog(FILE).unwrap();
        let before = template.clone();
        let mut after = template.clone();
        after[0].price = 2.8; // changed now
        after.push(CatalogItem {
            anchor: String::from("mate"),
            name: String::from("Mate"),
            price: 0.6,
            unit: None,
            tax: None,
        });
        let found = changes(&before, &after, &template);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].template.as_ref().unwrap().price, 2.5);
        assert!(found[0].describe().contains("2,50€ → 2,80€"));
        assert!(found[1].template.is_none());

        // already like that in the template: nothing to ask
        let template = after.clone();
        assert!(changes(&before, &after, &template).is_empty());
        // unchanged in this session: nothing to ask, even if the template differs (old projects)
        assert!(changes(&after, &after, &before).is_empty());

        let updated = update_template(FILE, &[found[0].item.clone(), found[1].item.clone()]).unwrap();
        let catalog = read_catalog(&updated).unwrap();
        assert_eq!(catalog.len(), 4);
        assert_eq!(catalog[0].price, 2.8);
        assert!(updated.contains("products:\n  *kaffee:\n    amount: 4\n"));
    }

    /// `ASCIII_ROUNDTRIP=dir cargo test --bin asciii roundtrip -- --ignored`: every project file in `dir`
    /// survives being read and written unchanged, as far as the library is concerned.
    #[test]
    #[ignore]
    fn roundtrip_real_files() {
        let dir = std::env::var("ASCIII_ROUNDTRIP").expect("set ASCIII_ROUNDTRIP");
        let mut checked = 0;
        for entry in walk(std::path::Path::new(&dir)) {
            let content = std::fs::read_to_string(&entry).unwrap();
            let Ok(offer) = read(&content) else {
                println!("skipped {}", entry.display());
                continue;
            };
            let written = write(&content, &offer).unwrap_or_else(|e| panic!("{}: {:#}", entry.display(), e));
            assert_eq!(written, content, "{} changed without a change", entry.display());
            // and a change goes through the checks of `write` (the library reads the same offer back)
            let mut changed = offer.clone();
            if let Some(line) = changed.lines.first_mut() {
                line.amount += 1.0;
            }
            changed.add_new(String::from("Testprodukt"), Some(String::from("stk")), 1.5, 2.0);
            if let Err(e) = write(&content, &changed) {
                panic!("{}: {:#}", entry.display(), e);
            }
            let before = Project::from_file_content(&content).unwrap();
            let after = Project::from_file_content(&written).unwrap();
            assert_eq!(before.sum_sold().unwrap(), after.sum_sold().unwrap(), "{}", entry.display());
            checked += 1;
        }
        println!("{} files checked", checked);
        assert!(checked > 0);
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else if path.extension().is_some_and(|e| e == "yml") {
                files.push(path);
            }
        }
        files
    }

    #[test]
    fn numbers() {
        assert_eq!(number(4.0), "4");
        assert_eq!(number(0.61), "0.61");
        assert_eq!(number(3.40), "3.4");
        assert_eq!(parse_number("2,50€"), Some(2.5));
        assert_eq!(money(2.5), "2,50€");
    }

    // ── templates ──────────────────────────────────────────────────────────

    fn templates() -> Vec<(&'static str, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        ["examples/tui-playground/projects/templates/default.tyml", "templates/default.tyml"]
            .into_iter()
            .map(|p| (p, std::fs::read_to_string(root.join(p)).unwrap()))
            .collect()
    }

    /// Lines of `after` that are not in `before` (and the other way round), as a count.
    fn changed_lines(before: &str, after: &str) -> usize {
        let a: Vec<&str> = before.lines().collect();
        let b: Vec<&str> = after.lines().collect();
        a.iter().filter(|l| !b.contains(l)).count() + b.iter().filter(|l| !a.contains(l)).count()
    }

    #[test]
    fn template_unchanged_is_written_byte_for_byte() {
        for (name, content) in templates() {
            let template = read_template(&content).unwrap();
            assert_eq!(write_template(&content, &template).unwrap(), content, "{}", name);
        }
    }

    #[test]
    fn template_edits_touch_only_their_lines() {
        for (name, content) in templates() {
            let original = read_template(&content).unwrap();

            // a new product with a starting amount
            let mut t = original.clone();
            let anchor = t.free_anchor("Kaffee");
            assert_eq!(anchor, "kaffee_2", "anchors stay unique");
            t.catalog.push(CatalogItem { anchor: anchor.clone(), name: String::from("Kaffee"), price: 2.9, unit: Some(String::from("1l")), tax: None });
            t.lines.push(Line { anchor: Some(anchor.clone()), name: String::new(), price: 0.0, unit: None, amount: 3.0, sold: None, returned: None, extra: Vec::new() });
            let written = write_template(&content, &t).unwrap();
            assert_eq!(read_template(&written).unwrap(), t, "{}", name);
            assert_eq!(changed_lines(&content, &written), 2, "{}: one catalog line, one product line", name);
            assert!(written.contains("&kaffee_2"));

            // rename and new price: the anchor stays
            let mut t = original.clone();
            let tee = t.catalog.iter_mut().find(|i| i.anchor == "tee").unwrap();
            tee.name = String::from("Schwarztee");
            tee.price = 1.95;
            let written = write_template(&content, &t).unwrap();
            assert_eq!(changed_lines(&content, &written), 2, "{}: only the tee line", name);
            assert!(written.contains("&tee") && written.contains("Schwarztee") && written.contains("1.95"));

            // removing a product of the starting offer removes it there too
            let mut t = original.clone();
            t.catalog.retain(|i| i.anchor != "kekse");
            t.lines.retain(|l| l.anchor.as_deref() != Some("kekse"));
            let written = write_template(&content, &t).unwrap();
            assert!(!written.contains("kekse"), "{}", name);
            assert_eq!(read_template(&written).unwrap(), t);

            // starting amount set, usage note cleared, product taken out of the starting offer
            let mut t = original.clone();
            let kaffee = t.lines.iter_mut().find(|l| l.anchor.as_deref() == Some("kaffee")).unwrap();
            kaffee.amount = 6.0;
            kaffee.sold = None;
            t.lines.retain(|l| l.anchor.as_deref() != Some("wasser_gross"));
            let written = write_template(&content, &t).unwrap();
            assert_eq!(read_template(&written).unwrap(), t, "{}", name);
            assert!(!written.contains("sold"), "{}", name);

            // comments and placeholders outside the sections stay byte for byte
            assert_eq!(placeholders(&content), placeholders(&written));
            assert_eq!(outside_sections(&content), outside_sections(&written));
            for line in content.lines().filter(|l| l.trim_start().starts_with('#') || l.contains("##")) {
                assert!(written.contains(line), "{}: lost {:?}", name, line);
            }

            // a project made from the edited template opens with the library and has this offer
            let project = filled_like_new_project(&written);
            let opened = Project::from_file_content(&project);
            assert!(opened.is_ok(), "{}", name);
            assert_eq!(read(&project).unwrap(), t, "{}", name);
        }
    }

    #[test]
    fn template_without_sections() {
        let content = "--- # template\nevent:\n  name: ##PROJECT-NAME##\n";
        let template = read_template(content).unwrap();
        assert!(template.catalog.is_empty() && template.lines.is_empty());
        // a product needs a products section to go into
        let mut t = template.clone();
        t.catalog.push(CatalogItem { anchor: String::from("tee"), name: String::from("Tee"), price: 1.0, unit: None, tax: None });
        assert!(write_template(content, &t).is_err());
    }
}
