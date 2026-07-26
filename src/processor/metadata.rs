use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use lopdf::{decode_text_string, text_string, Dictionary, Document, Object, ObjectId};
use serde::Serialize;
use time::{format_description::well_known::Rfc3339, Date, Month, OffsetDateTime, Time, UtcOffset};

use crate::cli::{
    MetadataClearArgs, MetadataCommand, MetadataField, MetadataSetArgs, MetadataShowArgs,
    MetadataSubcommand,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct PdfInfoMetadata {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    creation_date: Option<String>,
    modification_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MetadataReport {
    source: String,
    page_count: usize,
    pdf_version: String,
    info: PdfInfoMetadata,
    xmp: XmpReport,
    signatures: SignatureReport,
}

#[derive(Debug, Clone, Serialize)]
struct XmpReport {
    present: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SignatureReport {
    present: bool,
}

#[derive(Debug, Serialize)]
struct MetadataWriteReport {
    input: String,
    output: String,
    changed: Vec<String>,
    cleared: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct FieldUpdate {
    field: InfoField,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InfoField {
    Title,
    Author,
    Subject,
    Keywords,
    Creator,
    Producer,
    CreationDate,
    ModDate,
}

pub fn run(args: &MetadataCommand) -> anyhow::Result<()> {
    match &args.command {
        MetadataSubcommand::Show(args) => show(args),
        MetadataSubcommand::Set(args) => set(args),
        MetadataSubcommand::Clear(args) => clear(args),
    }
}

fn show(args: &MetadataShowArgs) -> anyhow::Result<()> {
    let report = load_report(&args.input)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_report(&report, args.verbose);
    }
    Ok(())
}

fn set(args: &MetadataSetArgs) -> anyhow::Result<()> {
    ensure_output_is_not_input(&args.input, &args.output)?;

    let mut updates = explicit_updates(args)?;
    if updates.is_empty() {
        bail!("metadata set requires at least one field such as --title or --author");
    }

    if args.mod_date.is_none() && !args.no_touch_mod_date {
        updates.push(FieldUpdate {
            field: InfoField::ModDate,
            value: format_pdf_date(OffsetDateTime::now_utc()),
        });
    }

    let input_report = load_report(&args.input)?;
    validate_signature_policy(&input_report, args.force_signed)?;
    let warnings = write_warnings(&input_report);

    let mut doc = load_document(&args.input)?;
    mutate_info_dictionary(&mut doc, |dict| {
        for update in &updates {
            dict.set(update.field.pdf_key(), text_string(&update.value));
        }
        Ok(())
    })?;

    save_document(&mut doc, &args.output)?;
    verify_updates(&args.output, &updates)?;

    let changed = unique_field_names(updates.iter().map(|update| update.field));
    let report = MetadataWriteReport {
        input: args.input.display().to_string(),
        output: args.output.display().to_string(),
        changed,
        cleared: Vec::new(),
        warnings,
    };
    print_write_report(&report, args.json, args.verbose)?;
    Ok(())
}

fn clear(args: &MetadataClearArgs) -> anyhow::Result<()> {
    ensure_output_is_not_input(&args.input, &args.output)?;

    let fields = expand_clear_fields(&args.fields)?;
    if fields.is_empty() {
        bail!("metadata clear requires at least one field");
    }

    let input_report = load_report(&args.input)?;
    validate_signature_policy(&input_report, args.force_signed)?;
    let warnings = write_warnings(&input_report);

    let mut doc = load_document(&args.input)?;
    mutate_info_dictionary(&mut doc, |dict| {
        for field in &fields {
            dict.remove(field.pdf_key());
        }
        Ok(())
    })?;

    save_document(&mut doc, &args.output)?;
    verify_cleared(&args.output, &fields)?;

    let cleared = unique_field_names(fields.iter().copied());
    let report = MetadataWriteReport {
        input: args.input.display().to_string(),
        output: args.output.display().to_string(),
        changed: Vec::new(),
        cleared,
        warnings,
    };
    print_write_report(&report, args.json, args.verbose)?;
    Ok(())
}

fn explicit_updates(args: &MetadataSetArgs) -> anyhow::Result<Vec<FieldUpdate>> {
    let mut updates = Vec::new();
    push_string_update(&mut updates, InfoField::Title, &args.title);
    push_string_update(&mut updates, InfoField::Author, &args.author);
    push_string_update(&mut updates, InfoField::Subject, &args.subject);
    push_string_update(&mut updates, InfoField::Keywords, &args.keywords);
    push_string_update(&mut updates, InfoField::Creator, &args.creator);
    push_string_update(&mut updates, InfoField::Producer, &args.producer);

    if let Some(value) = &args.creation_date {
        updates.push(FieldUpdate {
            field: InfoField::CreationDate,
            value: normalize_pdf_date(value).with_context(|| "invalid --creation-date")?,
        });
    }
    if let Some(value) = &args.mod_date {
        updates.push(FieldUpdate {
            field: InfoField::ModDate,
            value: normalize_pdf_date(value).with_context(|| "invalid --mod-date")?,
        });
    }

    Ok(updates)
}

fn push_string_update(updates: &mut Vec<FieldUpdate>, field: InfoField, value: &Option<String>) {
    if let Some(value) = value {
        updates.push(FieldUpdate {
            field,
            value: value.clone(),
        });
    }
}

fn load_report(path: &Path) -> anyhow::Result<MetadataReport> {
    let doc = load_document(path)?;
    let info = info_dictionary(&doc)
        .map(read_info_metadata)
        .unwrap_or_default();
    let page_count = doc.get_pages().len();
    Ok(MetadataReport {
        source: path.display().to_string(),
        page_count,
        pdf_version: doc.version.clone(),
        info,
        xmp: XmpReport {
            present: xmp_metadata_present(&doc),
        },
        signatures: SignatureReport {
            present: signature_fields_present(&doc),
        },
    })
}

fn load_document(path: &Path) -> anyhow::Result<Document> {
    let doc = Document::load(path).with_context(|| format!("failed to load {}", path.display()))?;
    if doc.is_encrypted() || doc.was_encrypted() {
        bail!(
            "{} is encrypted/password-protected; decrypt it before using `pdfp metadata` \
             (for example: qpdf --decrypt input.pdf decrypted.pdf)",
            path.display()
        );
    }
    Ok(doc)
}

fn read_info_metadata(dict: &Dictionary) -> PdfInfoMetadata {
    PdfInfoMetadata {
        title: decode_info_field(dict, b"Title"),
        author: decode_info_field(dict, b"Author"),
        subject: decode_info_field(dict, b"Subject"),
        keywords: decode_info_field(dict, b"Keywords"),
        creator: decode_info_field(dict, b"Creator"),
        producer: decode_info_field(dict, b"Producer"),
        creation_date: decode_info_field(dict, b"CreationDate"),
        modification_date: decode_info_field(dict, b"ModDate"),
    }
}

fn decode_info_field(dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key)
        .ok()
        .and_then(|object| decode_text_string(object).ok())
}

fn mutate_info_dictionary<F>(doc: &mut Document, mutate: F) -> anyhow::Result<()>
where
    F: FnOnce(&mut Dictionary) -> anyhow::Result<()>,
{
    let target = match doc.trailer.get(b"Info").ok().cloned() {
        Some(Object::Reference(id)) => InfoTarget::Object(id),
        Some(Object::Dictionary(_)) => InfoTarget::Trailer,
        Some(_) | None => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", id);
            InfoTarget::Object(id)
        }
    };

    match target {
        InfoTarget::Trailer => {
            let object = doc
                .trailer
                .get_mut(b"Info")
                .context("failed to access trailer Info dictionary")?;
            if !matches!(object, Object::Dictionary(_)) {
                *object = Object::Dictionary(Dictionary::new());
            }
            mutate(object.as_dict_mut()?)
        }
        InfoTarget::Object(id) => {
            doc.objects
                .entry(id)
                .or_insert_with(|| Object::Dictionary(Dictionary::new()));
            let object = doc
                .get_object_mut(id)
                .with_context(|| format!("failed to access Info object {} {}", id.0, id.1))?;
            if !matches!(object, Object::Dictionary(_)) {
                *object = Object::Dictionary(Dictionary::new());
            }
            mutate(object.as_dict_mut()?)
        }
    }
}

enum InfoTarget {
    Trailer,
    Object(ObjectId),
}

fn save_document(doc: &mut Document, output: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    doc.save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;
    Ok(())
}

fn verify_updates(output: &Path, updates: &[FieldUpdate]) -> anyhow::Result<()> {
    let report = load_report(output)?;
    for update in updates {
        let actual = update.field.value_from(&report.info);
        if actual != Some(update.value.as_str()) {
            bail!(
                "metadata verification failed for {} in {}",
                update.field.report_name(),
                output.display()
            );
        }
    }
    Ok(())
}

fn verify_cleared(output: &Path, fields: &[InfoField]) -> anyhow::Result<()> {
    let report = load_report(output)?;
    for field in fields {
        if field.value_from(&report.info).is_some() {
            bail!(
                "metadata verification failed to clear {} in {}",
                field.report_name(),
                output.display()
            );
        }
    }
    Ok(())
}

fn info_dictionary(doc: &Document) -> Option<&Dictionary> {
    let info = doc.trailer.get(b"Info").ok()?;
    let (_, object) = doc.dereference(info).ok()?;
    object.as_dict().ok()
}

fn catalog_dictionary(doc: &Document) -> Option<&Dictionary> {
    let root = doc.trailer.get(b"Root").ok()?;
    let (_, object) = doc.dereference(root).ok()?;
    object.as_dict().ok()
}

fn xmp_metadata_present(doc: &Document) -> bool {
    catalog_dictionary(doc).is_some_and(|catalog| catalog.has(b"Metadata"))
}

fn signature_fields_present(doc: &Document) -> bool {
    let Some(catalog) = catalog_dictionary(doc) else {
        return false;
    };

    if catalog.has(b"Perms") {
        return true;
    }

    let Some(acro_form) = catalog
        .get(b"AcroForm")
        .ok()
        .and_then(|object| dereference_dictionary(doc, object))
    else {
        return false;
    };

    acro_form
        .get(b"Fields")
        .ok()
        .and_then(|object| object.as_array().ok())
        .is_some_and(|fields| {
            fields
                .iter()
                .any(|field| signature_field_present(doc, field, 0))
        })
}

fn signature_field_present(doc: &Document, object: &Object, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let Some(dict) = dereference_dictionary(doc, object) else {
        return false;
    };
    if dict.get(b"FT").ok().and_then(|value| value.as_name().ok()) == Some(b"Sig") {
        return true;
    }
    dict.get(b"Kids")
        .ok()
        .and_then(|value| value.as_array().ok())
        .is_some_and(|kids| {
            kids.iter()
                .any(|kid| signature_field_present(doc, kid, depth + 1))
        })
}

fn dereference_dictionary<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    let (_, object) = doc.dereference(object).ok()?;
    object.as_dict().ok()
}

fn validate_signature_policy(report: &MetadataReport, force_signed: bool) -> anyhow::Result<()> {
    if report.signatures.present && !force_signed {
        bail!(
            "{} appears to contain signature fields; metadata writes can invalidate signatures. \
             Re-run with --force-signed to write anyway.",
            report.source
        );
    }
    Ok(())
}

fn write_warnings(report: &MetadataReport) -> Vec<String> {
    let mut warnings = Vec::new();
    if report.xmp.present {
        warnings.push(
            "XMP metadata is present; pdfp updates the document information dictionary only"
                .to_string(),
        );
    }
    if report.signatures.present {
        warnings.push(
            "signature fields are present; written output may invalidate signatures".to_string(),
        );
    }
    warnings
}

fn expand_clear_fields(fields: &[MetadataField]) -> anyhow::Result<Vec<InfoField>> {
    let mut expanded = BTreeSet::new();
    for field in fields {
        match field {
            MetadataField::Title => {
                expanded.insert(InfoField::Title);
            }
            MetadataField::Author => {
                expanded.insert(InfoField::Author);
            }
            MetadataField::Subject => {
                expanded.insert(InfoField::Subject);
            }
            MetadataField::Keywords => {
                expanded.insert(InfoField::Keywords);
            }
            MetadataField::Creator => {
                expanded.insert(InfoField::Creator);
            }
            MetadataField::Producer => {
                expanded.insert(InfoField::Producer);
            }
            MetadataField::CreationDate => {
                expanded.insert(InfoField::CreationDate);
            }
            MetadataField::ModDate => {
                expanded.insert(InfoField::ModDate);
            }
            MetadataField::All => {
                expanded.extend(InfoField::all());
            }
        }
    }
    Ok(expanded.into_iter().collect())
}

fn normalize_pdf_date(value: &str) -> anyhow::Result<String> {
    if value == "now" {
        return Ok(format_pdf_date(OffsetDateTime::now_utc()));
    }
    if value.starts_with("D:") {
        if valid_pdf_date(value) {
            return Ok(value.to_string());
        }
        bail!("expected a valid PDF date like D:20260519123000Z or D:20260519123000+08'00'");
    }
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .with_context(|| "expected `now`, RFC3339, or a PDF date beginning with D:")?;
    Ok(format_pdf_date(parsed))
}

fn valid_pdf_date(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("D:") else {
        return false;
    };
    let bytes = rest.as_bytes();
    if !matches!(bytes.len(), 14 | 15 | 21) || !bytes[..14].iter().all(u8::is_ascii_digit) {
        return false;
    }

    let year = four_digits(&bytes[0..4]).unwrap_or_default() as i32;
    let Some(month) = two_digits(&bytes[4..6]).and_then(|value| Month::try_from(value).ok()) else {
        return false;
    };
    let Some(day) = two_digits(&bytes[6..8]) else {
        return false;
    };
    let Some(hour) = two_digits(&bytes[8..10]) else {
        return false;
    };
    let Some(minute) = two_digits(&bytes[10..12]) else {
        return false;
    };
    let Some(second) = two_digits(&bytes[12..14]) else {
        return false;
    };
    if Date::from_calendar_date(year, month, day).is_err()
        || Time::from_hms(hour, minute, second).is_err()
    {
        return false;
    }

    match bytes.len() {
        14 => true,
        15 => bytes[14] == b'Z',
        21 => {
            matches!(bytes[14], b'+' | b'-')
                && bytes[17] == b'\''
                && bytes[20] == b'\''
                && two_digits(&bytes[15..17]).is_some_and(|hours| hours <= 23)
                && two_digits(&bytes[18..20]).is_some_and(|minutes| minutes <= 59)
        }
        _ => false,
    }
}

fn two_digits(bytes: &[u8]) -> Option<u8> {
    (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_digit))
        .then(|| (bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
}

fn four_digits(bytes: &[u8]) -> Option<u16> {
    (bytes.len() == 4 && bytes.iter().all(u8::is_ascii_digit)).then(|| {
        u16::from(bytes[0] - b'0') * 1000
            + u16::from(bytes[1] - b'0') * 100
            + u16::from(bytes[2] - b'0') * 10
            + u16::from(bytes[3] - b'0')
    })
}

fn format_pdf_date(date_time: OffsetDateTime) -> String {
    let base = format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        date_time.year(),
        u8::from(date_time.month()),
        date_time.day(),
        date_time.hour(),
        date_time.minute(),
        date_time.second()
    );
    let offset = date_time.offset();
    if offset == UtcOffset::UTC {
        return format!("D:{base}Z");
    }

    let seconds = offset.whole_seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let absolute = seconds.unsigned_abs();
    let hours = absolute / 3600;
    let minutes = (absolute % 3600) / 60;
    format!("D:{base}{sign}{hours:02}'{minutes:02}'")
}

fn print_human_report(report: &MetadataReport, verbose: bool) {
    println!("source: {}", report.source);
    println!("pages: {}", report.page_count);
    print_optional("title", &report.info.title);
    print_optional("author", &report.info.author);
    print_optional("subject", &report.info.subject);
    print_optional("keywords", &report.info.keywords);
    print_optional("creator", &report.info.creator);
    print_optional("producer", &report.info.producer);
    print_optional("creation date", &report.info.creation_date);
    print_optional("modification date", &report.info.modification_date);
    if verbose {
        println!("pdf version: {}", report.pdf_version);
        println!("xmp metadata: {}", report.xmp.present);
        println!("signature fields: {}", report.signatures.present);
    }
}

fn print_optional(label: &str, value: &Option<String>) {
    if let Some(value) = value {
        println!("{label}: {value}");
    }
}

fn print_write_report(
    report: &MetadataWriteReport,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    if verbose && !report.changed.is_empty() {
        eprintln!("changed: {}", report.changed.join(", "));
    }
    if verbose && !report.cleared.is_empty() {
        eprintln!("cleared: {}", report.cleared.join(", "));
    }
    println!("wrote {}", report.output);
    Ok(())
}

fn unique_field_names(fields: impl Iterator<Item = InfoField>) -> Vec<String> {
    fields
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|field| field.report_name().to_string())
        .collect()
}

fn ensure_output_is_not_input(input: &Path, output: &Path) -> anyhow::Result<()> {
    let input_abs = comparable_path(input);
    let output_abs = comparable_path(output);
    if input_abs == output_abs {
        bail!(
            "refusing to overwrite input PDF {}; choose a different -o path",
            input.display()
        );
    }
    Ok(())
}

fn comparable_path(path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(path) {
        return path;
    }

    let absolute = absolutize(path);
    let Some(file_name) = absolute.file_name() else {
        return absolute;
    };
    let Some(parent) = absolute.parent() else {
        return absolute;
    };
    match std::fs::canonicalize(parent) {
        Ok(parent) => parent.join(file_name),
        Err(_) => absolute,
    }
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

impl InfoField {
    fn all() -> [Self; 8] {
        [
            Self::Title,
            Self::Author,
            Self::Subject,
            Self::Keywords,
            Self::Creator,
            Self::Producer,
            Self::CreationDate,
            Self::ModDate,
        ]
    }

    fn pdf_key(self) -> &'static [u8] {
        match self {
            Self::Title => b"Title",
            Self::Author => b"Author",
            Self::Subject => b"Subject",
            Self::Keywords => b"Keywords",
            Self::Creator => b"Creator",
            Self::Producer => b"Producer",
            Self::CreationDate => b"CreationDate",
            Self::ModDate => b"ModDate",
        }
    }

    fn report_name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Author => "author",
            Self::Subject => "subject",
            Self::Keywords => "keywords",
            Self::Creator => "creator",
            Self::Producer => "producer",
            Self::CreationDate => "creation_date",
            Self::ModDate => "modification_date",
        }
    }

    fn value_from(self, metadata: &PdfInfoMetadata) -> Option<&str> {
        match self {
            Self::Title => metadata.title.as_deref(),
            Self::Author => metadata.author.as_deref(),
            Self::Subject => metadata.subject.as_deref(),
            Self::Keywords => metadata.keywords.as_deref(),
            Self::Creator => metadata.creator.as_deref(),
            Self::Producer => metadata.producer.as_deref(),
            Self::CreationDate => metadata.creation_date.as_deref(),
            Self::ModDate => metadata.modification_date.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_pdf_date_forms() {
        assert!(valid_pdf_date("D:20260519123000"));
        assert!(valid_pdf_date("D:20260519123000Z"));
        assert!(valid_pdf_date("D:20260519123000+08'00'"));
        assert!(!valid_pdf_date("D:20260519123000+0800"));
        assert!(!valid_pdf_date("D:2026051912300Z"));
        assert!(!valid_pdf_date("D:20261319123000Z"));
        assert!(!valid_pdf_date("D:20260230123000Z"));
        assert!(!valid_pdf_date("D:20260519243000Z"));
        assert!(!valid_pdf_date("D:20260519123000+24'00'"));
        assert!(!valid_pdf_date("D:20260519123💥"));
    }
}
