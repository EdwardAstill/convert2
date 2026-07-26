use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Document, Encoding, Object, ObjectId};

use crate::cli::{PageTextArgs, PdfTextFont, TextOrigin};
use crate::processor::page_range::parse_page_selection;

pub fn run(args: &PageTextArgs) -> anyhow::Result<()> {
    ensure_output_is_not_input(&args.input, &args.output)?;
    validate_args(args)?;

    let color = RgbColor::parse(&args.color)?;
    let line_height = args.line_height.unwrap_or(args.font_size * 1.2);
    if !line_height.is_finite() || line_height <= 0.0 {
        bail!("--line-height must be a finite number greater than 0");
    }
    let lines = encode_lines(&args.text)?;

    let mut doc = load_unencrypted_document(&args.input)?;
    let signatures_present = signature_fields_present(&doc);
    if signatures_present && !args.force_signed {
        bail!(
            "{} appears to contain signature fields; adding text can invalidate signatures. \
             Re-run with --force-signed to write anyway.",
            args.input.display()
        );
    }
    if signatures_present {
        eprintln!(
            "warning: signature fields are present; written output may invalidate signatures"
        );
    }

    let pages = doc.get_pages();
    let selected = parse_page_selection(&args.pages, pages.len())?;
    let font_id = add_standard_font(&mut doc, args.font);

    for page_index in selected {
        let page_num = (page_index + 1) as u32;
        let page_id = pages
            .get(&page_num)
            .copied()
            .with_context(|| format!("page {page_num} not found in {}", args.input.display()))?;
        let page_box = visible_page_box(&doc, page_id)
            .with_context(|| format!("failed to read page {page_num} bounds"))?;
        let font_resource = install_font_resource(&mut doc, page_id, font_id)
            .with_context(|| format!("failed to install font on page {page_num}"))?;
        let (x, y) = page_position(page_box, args.x, args.y, args.origin);
        if !x.is_finite() || !y.is_finite() {
            bail!("text position overflowed on page {page_num}; use smaller --x or --y values");
        }
        let content = overlay_content(
            &font_resource,
            args.font_size,
            color,
            x,
            y,
            line_height,
            &lines,
        )?;
        doc.add_page_contents(page_id, content)
            .with_context(|| format!("failed to add text to page {page_num}"))?;
    }

    save_document(&mut doc, &args.output)?;
    eprintln!("wrote {}", args.output.display());
    Ok(())
}

fn validate_args(args: &PageTextArgs) -> anyhow::Result<()> {
    if args.text.is_empty() {
        bail!("--text cannot be empty");
    }
    if !args.font_size.is_finite() || args.font_size <= 0.0 {
        bail!("--font-size must be a finite number greater than 0");
    }
    if !args.x.is_finite() || !args.y.is_finite() {
        bail!("--x and --y must be finite numbers");
    }
    Ok(())
}

fn encode_lines(text: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized
        .chars()
        .any(|character| character.is_control() && character != '\n')
    {
        bail!("--text may contain newlines but not other control characters");
    }

    let encoding = Encoding::SimpleEncoding(b"WinAnsiEncoding");
    normalized
        .split('\n')
        .map(|line| {
            let encoded = Document::encode_text(&encoding, line);
            let decoded = Document::decode_text(&encoding, &encoded)?;
            if decoded != line {
                bail!(
                    "text contains characters unsupported by the built-in PDF fonts; \
                     currently supported text uses Windows-1252 characters"
                );
            }
            Ok(encoded)
        })
        .collect()
}

fn add_standard_font(doc: &mut Document, font: PdfTextFont) -> ObjectId {
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => font.base_name(),
        "Encoding" => "WinAnsiEncoding",
    })
}

fn install_font_resource(
    doc: &mut Document,
    page_id: ObjectId,
    font_id: ObjectId,
) -> anyhow::Result<Vec<u8>> {
    let mut resources = match inherited_value(doc, page_id, b"Resources")? {
        Some(object) => object
            .as_dict()
            .context("page Resources entry is not a dictionary")?
            .clone(),
        None => Dictionary::new(),
    };

    let mut fonts = match resources.get(b"Font") {
        Ok(object) => {
            let (_, object) = doc.dereference(object)?;
            object
                .as_dict()
                .context("page Font resource is not a dictionary")?
                .clone()
        }
        Err(_) => Dictionary::new(),
    };

    let mut resource_name = b"PdfpText".to_vec();
    let mut suffix = 2usize;
    while fonts.get(&resource_name).is_ok() {
        resource_name = format!("PdfpText{suffix}").into_bytes();
        suffix += 1;
    }
    fonts.set(resource_name.clone(), font_id);
    resources.set("Font", fonts);

    doc.get_object_mut(page_id)
        .with_context(|| format!("page object {} {} is missing", page_id.0, page_id.1))?
        .as_dict_mut()?
        .set("Resources", resources);
    Ok(resource_name)
}

fn overlay_content(
    font_resource: &[u8],
    font_size: f32,
    color: RgbColor,
    x: f32,
    y: f32,
    line_height: f32,
    lines: &[Vec<u8>],
) -> anyhow::Result<Vec<u8>> {
    let mut operations = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "rg",
            vec![color.red.into(), color.green.into(), color.blue.into()],
        ),
        Operation::new("BT", vec![]),
        Operation::new(
            "Tf",
            vec![Object::Name(font_resource.to_vec()), font_size.into()],
        ),
    ];

    for (line_index, line) in lines.iter().enumerate() {
        let baseline_y = y - line_height * line_index as f32;
        if !baseline_y.is_finite() {
            anyhow::bail!("text position overflowed; use smaller --y or --line-height values");
        }
        operations.push(Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                x.into(),
                baseline_y.into(),
            ],
        ));
        operations.push(Operation::new(
            "Tj",
            vec![Object::string_literal(line.clone())],
        ));
    }

    operations.push(Operation::new("ET", vec![]));
    operations.push(Operation::new("Q", vec![]));
    Content { operations }.encode().map_err(Into::into)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PageBox {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn visible_page_box(doc: &Document, page_id: ObjectId) -> anyhow::Result<PageBox> {
    for key in [b"CropBox".as_slice(), b"MediaBox".as_slice()] {
        let Some(object) = inherited_value(doc, page_id, key)? else {
            continue;
        };
        let values = object
            .as_array()
            .with_context(|| format!("{} is not an array", String::from_utf8_lossy(key)))?;
        if values.len() != 4 {
            bail!(
                "{} must contain four coordinates",
                String::from_utf8_lossy(key)
            );
        }
        let page_box = PageBox {
            x0: values[0].as_float()?,
            y0: values[1].as_float()?,
            x1: values[2].as_float()?,
            y1: values[3].as_float()?,
        };
        if ![page_box.x0, page_box.y0, page_box.x1, page_box.y1]
            .iter()
            .all(|value| value.is_finite())
            || page_box.x1 <= page_box.x0
            || page_box.y1 <= page_box.y0
        {
            bail!("page box coordinates are invalid");
        }
        return Ok(page_box);
    }
    bail!("page has no inheritable CropBox or MediaBox")
}

fn page_position(page_box: PageBox, x: f32, y: f32, origin: TextOrigin) -> (f32, f32) {
    let page_x = page_box.x0 + x;
    let page_y = match origin {
        TextOrigin::BottomLeft => page_box.y0 + y,
        TextOrigin::TopLeft => page_box.y1 - y,
    };
    (page_x, page_y)
}

fn inherited_value(
    doc: &Document,
    page_id: ObjectId,
    key: &[u8],
) -> anyhow::Result<Option<Object>> {
    let mut current_id = page_id;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(current_id) {
            bail!(
                "cycle in PDF page tree at {} {}",
                current_id.0,
                current_id.1
            );
        }
        let node = doc.get_dictionary(current_id).with_context(|| {
            format!(
                "page tree object {} {} is invalid",
                current_id.0, current_id.1
            )
        })?;
        if let Ok(value) = node.get(key) {
            let (_, value) = doc.dereference(value)?;
            return Ok(Some(value.clone()));
        }
        let Ok(parent_id) = node.get(b"Parent").and_then(Object::as_reference) else {
            return Ok(None);
        };
        current_id = parent_id;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RgbColor {
    red: f32,
    green: f32,
    blue: f32,
}

impl RgbColor {
    fn parse(input: &str) -> anyhow::Result<Self> {
        let normalized = input.trim().to_ascii_lowercase();
        let named = match normalized.as_str() {
            "black" => Some([0.0, 0.0, 0.0]),
            "white" => Some([1.0, 1.0, 1.0]),
            "red" => Some([1.0, 0.0, 0.0]),
            "green" => Some([0.0, 0.5, 0.0]),
            "lime" => Some([0.0, 1.0, 0.0]),
            "blue" => Some([0.0, 0.0, 1.0]),
            "yellow" => Some([1.0, 1.0, 0.0]),
            "cyan" | "aqua" => Some([0.0, 1.0, 1.0]),
            "magenta" | "fuchsia" => Some([1.0, 0.0, 1.0]),
            "gray" | "grey" => Some([0.5, 0.5, 0.5]),
            "orange" => Some([1.0, 0.647, 0.0]),
            "purple" => Some([0.5, 0.0, 0.5]),
            _ => None,
        };
        if let Some([red, green, blue]) = named {
            return Ok(Self { red, green, blue });
        }

        let hex = normalized.strip_prefix('#').unwrap_or(&normalized);
        let channels = match hex.len() {
            3 if hex.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
                let values = hex.as_bytes();
                [
                    hex_nibble(values[0])? * 17,
                    hex_nibble(values[1])? * 17,
                    hex_nibble(values[2])? * 17,
                ]
            }
            6 if hex.bytes().all(|byte| byte.is_ascii_hexdigit()) => [
                hex_byte(&hex[0..2])?,
                hex_byte(&hex[2..4])?,
                hex_byte(&hex[4..6])?,
            ],
            _ => bail!(
                "invalid --color `{input}`; use a name such as red or a hex value such as #3366cc"
            ),
        };
        Ok(Self {
            red: f32::from(channels[0]) / 255.0,
            green: f32::from(channels[1]) / 255.0,
            blue: f32::from(channels[2]) / 255.0,
        })
    }
}

fn hex_nibble(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => bail!("invalid hexadecimal colour component"),
    }
}

fn hex_byte(value: &str) -> anyhow::Result<u8> {
    u8::from_str_radix(value, 16).context("invalid hexadecimal colour component")
}

impl PdfTextFont {
    fn base_name(self) -> &'static str {
        match self {
            Self::Helvetica => "Helvetica",
            Self::HelveticaBold => "Helvetica-Bold",
            Self::HelveticaOblique => "Helvetica-Oblique",
            Self::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Self::TimesRoman => "Times-Roman",
            Self::TimesBold => "Times-Bold",
            Self::TimesItalic => "Times-Italic",
            Self::TimesBoldItalic => "Times-BoldItalic",
            Self::Courier => "Courier",
            Self::CourierBold => "Courier-Bold",
            Self::CourierOblique => "Courier-Oblique",
            Self::CourierBoldOblique => "Courier-BoldOblique",
        }
    }
}

fn load_unencrypted_document(path: &Path) -> anyhow::Result<Document> {
    let doc = Document::load(path).with_context(|| format!("failed to load {}", path.display()))?;
    if doc.is_encrypted() || doc.was_encrypted() {
        bail!(
            "{} is encrypted/password-protected; decrypt it before using `pdfp page text` \
             (for example: qpdf --decrypt input.pdf decrypted.pdf)",
            path.display()
        );
    }
    Ok(doc)
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
        .and_then(|object| dereference_array(doc, object))
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
        .and_then(|value| dereference_array(doc, value))
        .is_some_and(|kids| {
            kids.iter()
                .any(|kid| signature_field_present(doc, kid, depth + 1))
        })
}

fn catalog_dictionary(doc: &Document) -> Option<&Dictionary> {
    let root = doc.trailer.get(b"Root").ok()?;
    dereference_dictionary(doc, root)
}

fn dereference_dictionary<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    let (_, object) = doc.dereference(object).ok()?;
    object.as_dict().ok()
}

fn dereference_array<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Vec<Object>> {
    let (_, object) = doc.dereference(object).ok()?;
    object.as_array().ok()
}

fn save_document(doc: &mut Document, output: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    doc.save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;
    Ok(())
}

fn ensure_output_is_not_input(input: &Path, output: &Path) -> anyhow::Result<()> {
    if comparable_path(input) == comparable_path(output) {
        bail!(
            "refusing to overwrite input PDF {}; choose a different -o path",
            input.display()
        );
    }
    Ok(())
}

fn comparable_path(path: &Path) -> PathBuf {
    if let Ok(path) = fs::canonicalize(path) {
        return path;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let Some(file_name) = absolute.file_name() else {
        return absolute;
    };
    let Some(parent) = absolute.parent() else {
        return absolute;
    };
    fs::canonicalize(parent)
        .map(|parent| parent.join(file_name))
        .unwrap_or(absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_hex_colours() {
        assert_eq!(
            RgbColor::parse("red").unwrap(),
            RgbColor {
                red: 1.0,
                green: 0.0,
                blue: 0.0
            }
        );
        assert_eq!(
            RgbColor::parse("#0f8").unwrap(),
            RgbColor {
                red: 0.0,
                green: 1.0,
                blue: 136.0 / 255.0
            }
        );
        assert!(RgbColor::parse("not-a-colour").is_err());
    }

    #[test]
    fn converts_top_left_position_to_pdf_coordinates() {
        let page_box = PageBox {
            x0: 10.0,
            y0: 20.0,
            x1: 610.0,
            y1: 820.0,
        };
        assert_eq!(
            page_position(page_box, 30.0, 40.0, TextOrigin::TopLeft),
            (40.0, 780.0)
        );
        assert_eq!(
            page_position(page_box, 30.0, 40.0, TextOrigin::BottomLeft),
            (40.0, 60.0)
        );
    }

    #[test]
    fn rejects_characters_unavailable_in_standard_fonts() {
        assert!(encode_lines("Hello €").is_ok());
        assert!(encode_lines("Hello Ω").is_err());
    }
}
