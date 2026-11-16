//! Setting single values in a project file, based on the YAML structure instead
//! of `##PLACEHOLDER##` text.
//!
//! Project files are hand-edited YAML with comments, so they are not
//! re-serialized: only the value on the line of the wanted key is replaced,
//! everything else stays byte for byte. A `##PLACEHOLDER##` left behind the old
//! value is dropped, other comments are kept. The result is parsed again before
//! anyone writes it.

use anyhow::{bail, Error};
use yaml_rust::{Yaml, YamlLoader};

/// Fields the TUI can fill in, with what they hold. All plain scalars on their
/// own line in the templates.
pub const FIELDS: [(&str, Kind); 10] = [
	("client/title", Kind::Text),
	("client/first_name", Kind::Text),
	("client/last_name", Kind::Text),
	("client/email", Kind::Text),
	("manager", Kind::Text),
	("offer/date", Kind::Date),
	("invoice/number", Kind::Number),
	("invoice/date", Kind::Date),
	("invoice/payed_date", Kind::Date),
	("hours/wages_date", Kind::Date),
];

/// The name of a field for people, also for the names the library uses in its
/// "missing" lists (`invoice.date`, `payed_date`, `client_addressing`). Unknown
/// paths are returned as they are.
pub fn label(path: &str) -> String {
	match path.trim().replace('.', "/").as_str() {
		"invoice/number" => lformat!("Invoice number"),
		"invoice/date" => lformat!("Invoice date"),
		"invoice/payed_date" | "payed_date" => lformat!("Paid on"),
		"invoice/official" => lformat!("Official invoice number"),
		"hours/wages_date" => lformat!("Wages paid on"),
		"hours/caterers" => lformat!("Staff"),
		"hours/salary" => lformat!("Hourly wage"),
		"hours/tax" => lformat!("Wage tax"),
		"offer/date" => lformat!("Offer date"),
		"offer/appendix" | "appendix" => lformat!("Offer appendix"),
		"client/title" => lformat!("Client title"),
		"client/first_name" => lformat!("Client first name"),
		"client/last_name" => lformat!("Client last name"),
		"client/email" => lformat!("Client e-mail"),
		"client/address" => lformat!("Client address"),
		"client_addressing" => lformat!("Salutation (comes from the client title)"),
		"manager" => lformat!("Manager"),
		"name" | "event/name" => lformat!("Event name"),
		"date" | "event/dates" => lformat!("Event date"),
		"format" => lformat!("File format version"),
		"client" => lformat!("Client"),
		"invoice" => lformat!("Invoice"),
		"offer" => lformat!("Offer"),
		"event" => lformat!("Event"),
		_ => path.to_owned(),
	}
}

/// [`label`]s of `paths`, comma separated, each once.
pub fn labels(paths: &[String]) -> String {
	let mut seen: Vec<String> = Vec::new();
	for path in paths {
		let label = label(path);
		if !seen.contains(&label) {
			seen.push(label);
		}
	}
	seen.join(", ")
}

/// What a field holds, for checking input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	Number,
	Date,
	Text,
}

/// The value at `path` (`a/b/c`) of the parsed document, if it is a non-empty
/// scalar.
pub fn value_at(
	content: &str,
	path: &str,
) -> Option<String> {
	let docs = YamlLoader::load_from_str(content).ok()?;
	let mut node = docs.first()?;
	for key in path.split('/') {
		node = &node[key];
	}
	match node {
		Yaml::String(s) if !s.trim().is_empty() => Some(s.clone()),
		Yaml::Integer(i) => Some(i.to_string()),
		Yaml::Real(r) => Some(r.clone()),
		Yaml::Boolean(b) => Some(b.to_string()),
		_ => None,
	}
}

/// Whether `text` is a whole `##KEYWORD##` placeholder (as used by the
/// templates).
fn is_placeholder(text: &str) -> bool {
	let text = text.trim();
	text.len() > 4
		&& text.starts_with("##")
		&& text.ends_with("##")
		&& text[2..text.len() - 2]
			.chars()
			.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
}

/// Splits `value  # comment` at the first ` #` outside of quotes.
fn split_comment(rest: &str) -> (&str, Option<&str>) {
	let mut quote = None;
	let bytes = rest.as_bytes();
	for (i, &b) in bytes.iter().enumerate() {
		match (quote, b) {
			(None, b'"') | (None, b'\'') => quote = Some(b),
			(Some(q), _) if b == q => quote = None,
			(None, b'#') if i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t' => {
				return (&rest[..i], Some(&rest[i..]));
			},
			_ => {},
		}
	}
	(rest, None)
}

/// `value` as a YAML string inside `{ … }`: like [`scalar`], but `,[]{}` need
/// quotes there too, and so do words YAML would read as a number or a boolean.
pub fn flow_scalar(value: &str) -> String {
	let looks_typed = value.parse::<f64>().is_ok()
		|| matches!(
			value.to_lowercase().as_str(),
			"yes" | "no" | "true" | "false" | "null" | "on" | "off" | "~"
		);
	if looks_typed || value.contains([',', '[', ']', '{', '}']) {
		format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
	} else {
		scalar(value)
	}
}

/// `value` as a YAML scalar: plain if that is safe, double-quoted otherwise.
fn scalar(value: &str) -> String {
	let plain = !value.is_empty()
		&& value.trim() == value
		&& !value.contains(": ")
		&& !value.contains(" #")
		&& !value.starts_with(|c: char| "-?:,[]{}#&*!|>'\"%@`".contains(c));
	if plain {
		value.to_owned()
	} else {
		format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
	}
}

/// Line number, indentation and key length of the line holding `path`, if the
/// file has it.
fn find_key(
	lines: &[&str],
	path: &str,
) -> Option<(usize, usize, usize)> {
	let target: Vec<&str> = path.split('/').collect();
	// (indent, key) of the mappings around the current line
	let mut stack: Vec<(usize, String)> = Vec::new();
	// inside a `key: |` block: lines indented deeper than this belong to the text
	let mut block_indent: Option<usize> = None;
	let mut found = None;

	for (number, line) in lines.iter().enumerate() {
		let text = line.trim_end_matches(['\n', '\r']);
		let trimmed = text.trim_start();
		let indent = text.len() - trimmed.len();
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		if let Some(block) = block_indent {
			if indent > block {
				continue;
			}
			block_indent = None;
		}
		// list items and documents markers end the mappings we can address
		if trimmed.starts_with("- ")
			|| trimmed == "-"
			|| trimmed.starts_with("---")
			|| trimmed.starts_with("...")
		{
			stack.retain(|(i, _)| *i < indent);
			stack.push((indent, String::from("-")));
			continue;
		}
		let Some((key, rest)) = trimmed.split_once(':') else {
			continue;
		};
		if key.contains(' ') && !key.starts_with('"') {
			continue;
		}
		stack.retain(|(i, _)| *i < indent);
		stack.push((indent, key.trim_matches('"').to_owned()));

		let (value, _) = split_comment(rest);
		if matches!(value.trim(), "|" | ">" | "|-" | ">-" | "|+" | ">+") {
			block_indent = Some(indent);
		}
		if stack.len() == target.len() && stack.iter().zip(&target).all(|((_, k), t)| k == t) {
			found = Some((number, indent, key.len()));
			break;
		}
	}
	found
}

/// The fields of [`FIELDS`] this file has, with their current value.
pub fn fields_in(content: &str) -> Vec<(&'static str, Kind, Option<String>)> {
	let lines: Vec<&str> = content.split_inclusive('\n').collect();
	FIELDS
		.iter()
		.filter(|(path, _)| find_key(&lines, path).is_some())
		.map(|&(path, kind)| (path, kind, value_at(content, path)))
		.collect()
}

/// `##KEYWORD##` placeholders that ended up in values (not in comments), e.g.
/// in a text block. These are printed into the PDF as they are.
pub fn leftover_placeholders(content: &str) -> Vec<String> {
	fn walk(
		node: &Yaml,
		found: &mut Vec<String>,
	) {
		match node {
			Yaml::String(s) => {
				let mut rest = s.as_str();
				while let Some(start) = rest.find("##") {
					let after = &rest[start + 2..];
					let Some(end) = after.find("##") else { break };
					let word = &after[..end];
					if !word.is_empty()
						&& word
							.chars()
							.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
					{
						let word = word.to_owned();
						if !found.contains(&word) {
							found.push(word);
						}
					}
					rest = &after[end + 2..];
				}
			},
			Yaml::Array(items) => items.iter().for_each(|i| walk(i, found)),
			Yaml::Hash(map) => map.values().for_each(|v| walk(v, found)),
			_ => {},
		}
	}
	let mut found = Vec::new();
	if let Ok(docs) = YamlLoader::load_from_str(content) {
		docs.iter().for_each(|d| walk(d, &mut found));
	}
	found
}

/// Sets `path` to `value`, adding `key: value` to its mapping if the key is not
/// there yet (as the last line of the parent, e.g. `accepted:` under `offer:`).
pub fn set_or_add(
	content: &str,
	path: &str,
	value: &str,
) -> Result<String, Error> {
	let lines: Vec<&str> = content.split_inclusive('\n').collect();
	if find_key(&lines, path).is_some() {
		return set_value(content, path, value);
	}
	let Some((parent, key)) = path.rsplit_once('/') else {
		bail!(lformat!(
			"{} is not in the project file, add it with the editor (e)",
			label(path)
		));
	};
	let Some((number, indent, _)) = find_key(&lines, parent) else {
		bail!(lformat!(
			"{} is not in the project file, add it with the editor (e)",
			label(parent)
		));
	};
	// the children of the parent: the following lines indented deeper
	let mut last = number;
	let mut child_indent = None;
	for (n, line) in lines.iter().enumerate().skip(number + 1) {
		let text = line.trim_end_matches(['\n', '\r']);
		let trimmed = text.trim_start();
		if trimmed.is_empty() || trimmed.starts_with('#') {
			continue;
		}
		let this = text.len() - trimmed.len();
		if this <= indent {
			break;
		}
		child_indent.get_or_insert(this);
		last = n;
	}
	let new_line = format!(
		"{}{}: {}\n",
		" ".repeat(child_indent.unwrap_or(indent + 2)),
		key,
		scalar(value)
	);
	let mut result = String::with_capacity(content.len() + new_line.len());
	for (n, line) in lines.iter().enumerate() {
		result.push_str(line);
		if n == last {
			if !line.ends_with('\n') {
				result.push('\n');
			}
			result.push_str(&new_line);
		}
	}
	if value_at(&result, path).as_deref() != Some(value) {
		bail!(lformat!(
			"{} could not be set safely, change it with the editor (e)",
			label(path)
		));
	}
	Ok(result)
}

/// Removes the line `key: value` of `path`; nothing happens if it is not there.
pub fn remove_key(
	content: &str,
	path: &str,
) -> Result<String, Error> {
	let lines: Vec<&str> = content.split_inclusive('\n').collect();
	let Some((number, _, _)) = find_key(&lines, path) else {
		return Ok(content.to_owned());
	};
	let result: String = lines
		.iter()
		.enumerate()
		.filter(|(n, _)| *n != number)
		.map(|(_, l)| *l)
		.collect();
	if YamlLoader::load_from_str(&result).is_err() || value_at(&result, path).is_some() {
		bail!(lformat!(
			"{} could not be removed safely, change it with the editor (e)",
			label(path)
		));
	}
	Ok(result)
}

/// Replaces the value of `path` in `content`. The key must already exist as
/// `key: value` on one line.
pub fn set_value(
	content: &str,
	path: &str,
	value: &str,
) -> Result<String, Error> {
	let lines: Vec<&str> = content.split_inclusive('\n').collect();
	let Some((number, indent, key_len)) = find_key(&lines, path) else {
		bail!(lformat!(
			"{} is not in the project file, add it with the editor (e)",
			label(path)
		));
	};
	let line = lines[number];
	let ending = &line[line.trim_end_matches(['\n', '\r']).len()..];
	let rest = &line.trim_end_matches(['\n', '\r'])[indent + key_len + 1..];
	let (old, comment) = split_comment(rest);
	if matches!(old.trim(), "|" | ">" | "|-" | ">-" | "|+" | ">+") {
		bail!(lformat!(
			"{} is a text block, change it with the editor (e)",
			label(path)
		));
	}
	// keep the column the value started in, so aligned files stay aligned
	let gap = rest.len() - rest.trim_start().len();
	let gap = if gap == 0 || rest.trim().is_empty() {
		1.max(gap)
	} else {
		gap
	};
	let mut new_line = format!(
		"{}{}:{}{}",
		&line[..indent],
		&line[indent..indent + key_len],
		" ".repeat(gap),
		scalar(value)
	);
	if let Some(comment) = comment.filter(|c| !is_placeholder(c)) {
		new_line.push_str("  ");
		new_line.push_str(comment.trim());
	}
	new_line.push_str(ending);

	let mut result = String::with_capacity(content.len() + value.len());
	for (n, l) in lines.iter().enumerate() {
		result.push_str(if n == number { &new_line } else { l });
	}

	// the edit must keep the file valid and actually set the value
	if YamlLoader::load_from_str(&result).is_err() {
		bail!(lformat!(
			"setting {} would break the project file, nothing changed",
			label(path)
		));
	}
	if value_at(&result, path).as_deref() != Some(value) {
		bail!(lformat!(
			"{} could not be set safely, change it with the editor (e)",
			label(path)
		));
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;

	const FILE: &str = "\
client:
  title:      Herr # Frau, Professor
  first_name: ##CLIENT-FIRST-NAME##
  address: |
    number: inside a text block
    ##CLIENT-ADDRESS##

event:
  dates:
  - begin: 01.06.2026
    number: 7

invoice:
  number:     ##INVOICE-NUMBER##
  date:       ##INVOICE-DATE##
  official:   # used if you to set an official invoice number

manager: Somebody
";

	#[test]
	fn fills_empty_values_and_drops_placeholders() {
		let filled = set_value(FILE, "invoice/number", "42").unwrap();
		assert!(filled.contains("  number:     42\n"));
		assert_eq!(value_at(&filled, "invoice/number").as_deref(), Some("42"));
		// nothing else changed
		assert_eq!(
			filled.replace("  number:     42\n", "  number:     ##INVOICE-NUMBER##\n"),
			FILE
		);

		let filled = set_value(FILE, "client/first_name", "Björn").unwrap();
		assert_eq!(
			value_at(&filled, "client/first_name").as_deref(),
			Some("Björn")
		);
	}

	#[test]
	fn keeps_real_comments_and_replaces_existing_values() {
		let filled = set_value(FILE, "client/title", "Frau").unwrap();
		assert!(filled.contains("  title:      Frau  # Frau, Professor\n"));

		let filled = set_value(FILE, "invoice/official", "R-2026-7").unwrap();
		assert!(filled.contains("official:   R-2026-7  # used if you"));
	}

	#[test]
	fn values_typed_before_a_placeholder_are_replaced_not_doubled() {
		// what an editor session can leave behind: the value, and the placeholder as a
		// comment
		let edited = FILE.replace("##INVOICE-NUMBER##", "1    ##INVOICE-NUMBER##");
		assert_eq!(value_at(&edited, "invoice/number").as_deref(), Some("1"));
		let filled = set_value(&edited, "invoice/number", "43").unwrap();
		assert!(filled.contains("  number:     43\n"));
	}

	#[test]
	fn ignores_text_blocks_and_lists() {
		// `number:` inside the address block and inside the dates list are not
		// invoice/number
		let filled = set_value(FILE, "invoice/number", "5").unwrap();
		assert!(filled.contains("    number: inside a text block\n"));
		assert!(filled.contains("    number: 7\n"));
		assert!(set_value(FILE, "client/address", "x").is_err());
	}

	#[test]
	fn quotes_when_needed_and_refuses_missing_keys() {
		let filled = set_value(FILE, "manager", "Ben: the #1").unwrap();
		assert_eq!(value_at(&filled, "manager").as_deref(), Some("Ben: the #1"));
		assert!(set_value(FILE, "hours/wages_date", "01.01.2026").is_err());
	}

	#[test]
	fn adds_and_removes_keys() {
		let file = "offer:\n  date: 01.09.2026\n  appendix: 1\n\n# money\ninvoice:\n  number: 3\n";
		let added = set_or_add(file, "offer/accepted", "true").unwrap();
		assert_eq!(added, "offer:\n  date: 01.09.2026\n  appendix: 1\n  accepted: true\n\n# money\ninvoice:\n  number: 3\n");
		// already there: changed in place
		assert_eq!(
			set_or_add(&added, "offer/accepted", "false").unwrap(),
			added.replace("accepted: true", "accepted: false")
		);
		assert_eq!(remove_key(&added, "offer/accepted").unwrap(), file);
		assert_eq!(remove_key(file, "offer/accepted").unwrap(), file);
		assert!(set_or_add(file, "nothing/here", "x").is_err());
	}

	#[test]
	fn every_field_has_a_human_label() {
		for (path, _) in FIELDS {
			let label = label(path);
			assert_ne!(label, path, "no label for {}", path);
			assert!(!label.contains('/'), "{} looks like a path", label);
		}
		// the library's names for the same fields
		assert_eq!(label("invoice.date"), label("invoice/date"));
		assert_eq!(label("payed_date"), label("invoice/payed_date"));
		assert_eq!(
			labels(&[String::from("client/title"), String::from("client/title")]),
			label("client/title")
		);
		assert_eq!(label("something/else"), "something/else");
	}

	#[test]
	fn lists_fields_and_leftover_placeholders() {
		let fields = fields_in(FILE);
		assert!(fields
			.iter()
			.any(|(p, _, v)| *p == "invoice/number" && v.is_none()));
		assert!(fields
			.iter()
			.any(|(p, _, v)| *p == "manager" && v.as_deref() == Some("Somebody")));
		assert!(!fields.iter().any(|(p, _, _)| *p == "hours/wages_date"));
		// only the one inside the address text, the others are comments
		assert_eq!(
			leftover_placeholders(FILE),
			vec![String::from("CLIENT-ADDRESS")]
		);
	}
}
