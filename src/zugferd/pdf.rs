//! Turns the PDF that pdflatex made into a PDF/A-3b with the invoice XML attached (Factur-X / ZUGFeRD).
//!
//! What is added:
//! * `factur-x.xml` as embedded file with `AFRelationship /Alternative`, listed in the catalog's `/AF`
//! * XMP metadata: PDF/A identification, the Factur-X schema (with its PDF/A extension schema description)
//!   and a copy of the document info (validators check that both agree)
//! * an sRGB output intent
//!
//! What cannot be fixed here: content PDF/A forbids (fonts that are not embedded, transparency, …).
//! pdflatex embeds its fonts by default.

use std::path::Path;

use anyhow::{Context, Error};
use lopdf::{decode_text_string, dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// Name of the attachment, fixed by the standard.
pub const XML_NAME: &str = "factur-x.xml";

/// sRGB, Compact-ICC-Profiles by Clinton Ingram (CC0), see `ICC-LICENSE.md`.
const SRGB_ICC: &[u8] = include_bytes!("sRGB-v2-micro.icc");

fn literal(text: &str) -> Object {
    Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
}

fn escape(text: &str) -> String {
    super::xml::escape(text)
}

/// `D:20260120123456+01'00'` → `2026-01-20T12:34:56+01:00`
pub fn xmp_date(pdf_date: &str) -> Option<String> {
    let text = pdf_date.trim().trim_start_matches("D:");
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |from: usize| digits.get(from..from + 2);
    let mut out = digits[..4].to_string();
    if let Some(month) = part(4) {
        out += &format!("-{}", month);
        if let Some(day) = part(6) {
            out += &format!("-{}", day);
            if let (Some(hour), Some(minute)) = (part(8), part(10)) {
                out += &format!("T{}:{}", hour, minute);
                if let Some(second) = part(12) {
                    out += &format!(":{}", second);
                }
                let zone = &text[digits.len()..];
                if zone.starts_with('Z') {
                    out.push('Z');
                } else if zone.starts_with('+') || zone.starts_with('-') {
                    let numbers: String = zone[1..].chars().filter(char::is_ascii_digit).collect();
                    if numbers.len() >= 2 {
                        let minutes = numbers.get(2..4).unwrap_or("00");
                        out += &format!("{}{}:{}", &zone[..1], &numbers[..2], minutes);
                    }
                }
            }
        }
    }
    Some(out)
}

/// The document info entries, decoded.
#[derive(Debug, Default)]
struct Info {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    created: Option<String>,
    modified: Option<String>,
}

fn read_info(doc: &Document) -> Info {
    let dict = match doc.trailer.get(b"Info").and_then(|info| doc.dereference(info)) {
        Ok((_, Object::Dictionary(dict))) => dict,
        _ => return Info::default(),
    };
    let text = |key: &[u8]| {
        dict.get(key)
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| decode_text_string(o).ok())
            .filter(|s| !s.trim().is_empty())
    };
    Info {
        title: text(b"Title"),
        author: text(b"Author"),
        subject: text(b"Subject"),
        keywords: text(b"Keywords"),
        creator: text(b"Creator"),
        producer: text(b"Producer"),
        created: text(b"CreationDate"),
        modified: text(b"ModDate"),
    }
}

fn xmp(info: &Info, now: &str) -> String {
    let mut x = String::from("<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    x += "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n";
    x += "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n";

    x += "<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\n";
    x += "  <pdfaid:part>3</pdfaid:part>\n  <pdfaid:conformance>B</pdfaid:conformance>\n";
    x += "</rdf:Description>\n";

    x += "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n";
    x += "  <dc:format>application/pdf</dc:format>\n";
    if let Some(title) = &info.title {
        x += &format!(
            "  <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
            escape(title)
        );
    }
    if let Some(author) = &info.author {
        x += &format!(
            "  <dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>\n",
            escape(author)
        );
    }
    if let Some(subject) = &info.subject {
        x += &format!(
            "  <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
            escape(subject)
        );
    }
    x += "</rdf:Description>\n";

    x += "<rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">\n";
    if let Some(producer) = &info.producer {
        x += &format!("  <pdf:Producer>{}</pdf:Producer>\n", escape(producer));
    }
    if let Some(keywords) = &info.keywords {
        x += &format!("  <pdf:Keywords>{}</pdf:Keywords>\n", escape(keywords));
    }
    x += "</rdf:Description>\n";

    x += "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">\n";
    if let Some(creator) = &info.creator {
        x += &format!("  <xmp:CreatorTool>{}</xmp:CreatorTool>\n", escape(creator));
    }
    if let Some(created) = info.created.as_deref().and_then(xmp_date) {
        x += &format!("  <xmp:CreateDate>{}</xmp:CreateDate>\n", created);
    }
    if let Some(modified) = info.modified.as_deref().and_then(xmp_date) {
        x += &format!("  <xmp:ModifyDate>{}</xmp:ModifyDate>\n", modified);
    }
    x += &format!("  <xmp:MetadataDate>{}</xmp:MetadataDate>\n", now);
    x += "</rdf:Description>\n";

    x += "<rdf:Description rdf:about=\"\" xmlns:fx=\"urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#\">\n";
    x += "  <fx:DocumentType>INVOICE</fx:DocumentType>\n";
    x += &format!("  <fx:DocumentFileName>{}</fx:DocumentFileName>\n", XML_NAME);
    x += "  <fx:Version>1.0</fx:Version>\n";
    x += "  <fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>\n";
    x += "</rdf:Description>\n";

    // PDF/A only accepts XMP properties it knows, so the Factur-X schema has to describe itself
    x += concat!(
        "<rdf:Description rdf:about=\"\"",
        " xmlns:pdfaExtension=\"http://www.aiim.org/pdfa/ns/extension/\"",
        " xmlns:pdfaSchema=\"http://www.aiim.org/pdfa/ns/schema#\"",
        " xmlns:pdfaProperty=\"http://www.aiim.org/pdfa/ns/property#\">\n",
        "  <pdfaExtension:schemas><rdf:Bag><rdf:li rdf:parseType=\"Resource\">\n",
        "    <pdfaSchema:schema>Factur-X PDFA Extension Schema</pdfaSchema:schema>\n",
        "    <pdfaSchema:namespaceURI>urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#</pdfaSchema:namespaceURI>\n",
        "    <pdfaSchema:prefix>fx</pdfaSchema:prefix>\n",
        "    <pdfaSchema:property><rdf:Seq>\n"
    );
    for (name, description) in [
        ("DocumentFileName", "The name of the embedded XML document"),
        ("DocumentType", "The type of the hybrid document in capital letters, e.g. INVOICE or ORDER"),
        ("Version", "The actual version of the standard applying to the embedded XML document"),
        ("ConformanceLevel", "The conformance level of the embedded XML document"),
    ] {
        x += &format!(
            concat!(
                "      <rdf:li rdf:parseType=\"Resource\">",
                "<pdfaProperty:name>{}</pdfaProperty:name>",
                "<pdfaProperty:valueType>Text</pdfaProperty:valueType>",
                "<pdfaProperty:category>external</pdfaProperty:category>",
                "<pdfaProperty:description>{}</pdfaProperty:description>",
                "</rdf:li>\n"
            ),
            name, description
        );
    }
    x += "    </rdf:Seq></pdfaSchema:property>\n";
    x += "  </rdf:li></rdf:Bag></pdfaExtension:schemas>\n";
    x += "</rdf:Description>\n";

    x += "</rdf:RDF>\n</x:xmpmeta>\n";
    // padding so the metadata can be edited in place later
    x += &format!("{}\n", " ".repeat(2000));
    x += "<?xpacket end=\"w\"?>";
    x
}

/// The catalog's `/Names` dictionary, made direct so it can be changed.
fn names_dict(doc: &Document, catalog: &Dictionary) -> Dictionary {
    match catalog.get(b"Names").and_then(|names| doc.dereference(names)) {
        Ok((_, Object::Dictionary(dict))) => dict.clone(),
        _ => Dictionary::new(),
    }
}

/// Attaches `xml` as `factur-x.xml` and makes the PDF a PDF/A-3b, in memory.
pub fn embed_into(doc: &mut Document, xml: &[u8], now: chrono::DateTime<chrono::Utc>) -> Result<(), Error> {
    let pdf_now = now.format("D:%Y%m%d%H%M%SZ").to_string();
    let xmp_now = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let info = read_info(doc);

    let mut file = Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Subtype" => Object::Name(b"text/xml".to_vec()),
            "Params" => dictionary! {
                "ModDate" => literal(&pdf_now),
                "Size" => xml.len() as i64,
            },
        },
        xml.to_vec(),
    );
    // the XML stays readable with any tool
    file = file.with_compression(false);
    let file_id = doc.add_object(file);

    let spec_id = doc.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => literal(XML_NAME),
        "UF" => literal(XML_NAME),
        "Desc" => literal("Factur-X/ZUGFeRD invoice"),
        "AFRelationship" => "Alternative",
        "EF" => dictionary! { "F" => file_id, "UF" => file_id },
    });

    let mut metadata = Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        xmp(&info, &xmp_now).into_bytes(),
    );
    // PDF/A: metadata streams must not be compressed
    metadata = metadata.with_compression(false);
    let metadata_id = doc.add_object(metadata);

    let icc_id = doc.add_object(
        Stream::new(dictionary! { "N" => 3 }, SRGB_ICC.to_vec()).with_compression(false),
    );
    let intent_id = doc.add_object(dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFA1",
        "OutputConditionIdentifier" => literal("sRGB IEC61966-2.1"),
        "Info" => literal("sRGB IEC61966-2.1"),
        "RegistryName" => literal("http://www.color.org"),
        "DestOutputProfile" => icc_id,
    });

    // keep other attachments, drop an earlier factur-x.xml
    let catalog = doc.catalog().context("PDF has no catalog")?.clone();
    let mut names = names_dict(doc, &catalog);
    let mut embedded: Vec<Object> = match names.get(b"EmbeddedFiles").and_then(|e| doc.dereference(e)) {
        Ok((_, Object::Dictionary(tree))) => tree
            .get(b"Names")
            .and_then(Object::as_array)
            .map(|entries| {
                entries
                    .chunks(2)
                    .filter(|pair| pair[0].as_str().map(|n| n != XML_NAME.as_bytes()).unwrap_or(true))
                    .flatten()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    embedded.push(literal(XML_NAME));
    embedded.push(Object::Reference(spec_id));
    // name trees must be sorted
    let mut pairs: Vec<(Object, Object)> = embedded
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect();
    pairs.sort_by(|a, b| a.0.as_str().unwrap_or(b"").cmp(b.0.as_str().unwrap_or(b"")));
    let other_specs: Vec<ObjectId> = pairs
        .iter()
        .filter_map(|(_, spec)| spec.as_reference().ok())
        .filter(|id| *id != spec_id)
        .collect();
    names.set(
        "EmbeddedFiles",
        dictionary! { "Names" => pairs.into_iter().flat_map(|(n, s)| [n, s]).collect::<Vec<_>>() },
    );

    let mut associated: Vec<Object> = other_specs
        .into_iter()
        .filter(|id| {
            // only files that are already associated stay in /AF
            catalog
                .get(b"AF")
                .and_then(Object::as_array)
                .map(|af| af.contains(&Object::Reference(*id)))
                .unwrap_or(false)
        })
        .map(Object::Reference)
        .collect();
    associated.push(Object::Reference(spec_id));

    let catalog = doc.catalog_mut().context("PDF has no catalog")?;
    catalog.set("Names", names);
    catalog.set("AF", associated);
    catalog.set("Metadata", metadata_id);
    catalog.set("OutputIntents", vec![Object::Reference(intent_id)]);
    Ok(())
}

/// Attaches `xml` to the PDF at `path` (in place).
pub fn embed(path: &Path, xml: &[u8]) -> Result<(), Error> {
    let mut doc = Document::load(path).with_context(|| format!("cannot read {}", path.display()))?;
    if doc.is_encrypted() {
        anyhow::bail!("{} is encrypted", path.display());
    }
    embed_into(&mut doc, xml, chrono::Utc::now())?;
    let tmp = path.with_extension("zugferd.pdf");
    doc.save(&tmp).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(
            xmp_date("D:20260120123456+01'00'").as_deref(),
            Some("2026-01-20T12:34:56+01:00")
        );
        assert_eq!(xmp_date("D:20260120123456Z").as_deref(), Some("2026-01-20T12:34:56Z"));
        assert_eq!(xmp_date("D:20260120").as_deref(), Some("2026-01-20"));
        assert_eq!(xmp_date("D:2026").as_deref(), Some("2026"));
        assert_eq!(xmp_date("garbage"), None);
    }

    /// a tiny one page PDF, as pdflatex would make it (with an info dictionary)
    fn sample_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        let info_id = doc.add_object(dictionary! {
            "Producer" => literal("pdfTeX-1.40.26"),
            "Creator" => literal("TeX"),
            "Title" => lopdf::text_string("Rechnung Café"),
            "CreationDate" => literal("D:20260120123456+01'00'"),
            "ModDate" => literal("D:20260120123456+01'00'"),
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn embeds_and_reads_back() {
        let xml = b"<?xml version=\"1.0\"?><rsm:CrossIndustryInvoice/>";
        let mut doc = Document::load_mem(&sample_pdf()).unwrap();
        embed_into(&mut doc, xml, chrono::Utc::now()).unwrap();
        // twice: still one factur-x.xml
        embed_into(&mut doc, xml, chrono::Utc::now()).unwrap();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();

        let doc = Document::load_mem(&bytes).unwrap();
        let catalog = doc.catalog().unwrap();

        let af = catalog.get(b"AF").unwrap().as_array().unwrap();
        assert_eq!(af.len(), 1);
        let spec = doc.get_dictionary(af[0].as_reference().unwrap()).unwrap();
        assert_eq!(spec.get(b"AFRelationship").unwrap().as_name().unwrap(), b"Alternative");
        assert_eq!(spec.get(b"F").unwrap().as_str().unwrap(), XML_NAME.as_bytes());
        let ef = spec.get(b"EF").unwrap().as_dict().unwrap();
        let file = doc.get_object(ef.get(b"F").unwrap().as_reference().unwrap()).unwrap();
        let file = file.as_stream().unwrap();
        assert_eq!(file.get_plain_content().unwrap(), xml.to_vec());
        assert_eq!(file.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"text/xml");

        let names = catalog.get(b"Names").unwrap().as_dict().unwrap();
        let tree = names.get(b"EmbeddedFiles").unwrap().as_dict().unwrap();
        assert_eq!(tree.get(b"Names").unwrap().as_array().unwrap().len(), 2);

        let metadata = doc.get_object(catalog.get(b"Metadata").unwrap().as_reference().unwrap()).unwrap();
        let metadata = metadata.as_stream().unwrap();
        assert!(!metadata.dict.has(b"Filter"));
        let xmp = String::from_utf8(metadata.content.clone()).unwrap();
        assert!(xmp.contains("<pdfaid:part>3</pdfaid:part>"));
        assert!(xmp.contains("<fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>"));
        // the info dictionary is mirrored
        assert!(xmp.contains("<pdf:Producer>pdfTeX-1.40.26</pdf:Producer>"));
        assert!(xmp.contains(">Rechnung Café</rdf:li>"));
        assert!(xmp.contains("<xmp:CreateDate>2026-01-20T12:34:56+01:00</xmp:CreateDate>"));

        let intents = catalog.get(b"OutputIntents").unwrap().as_array().unwrap();
        let intent = doc.get_dictionary(intents[0].as_reference().unwrap()).unwrap();
        assert_eq!(intent.get(b"S").unwrap().as_name().unwrap(), b"GTS_PDFA1");
        let icc = doc
            .get_object(intent.get(b"DestOutputProfile").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(&icc.as_stream().unwrap().content[36..40], b"acsp");
    }
}
