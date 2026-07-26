//! CLI integration tests for page-level PDF operations.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::content::Operation;
use lopdf::{
    dictionary, Document, Encoding, EncryptionState, EncryptionVersion, Object, Permissions,
    Stream, StringFormat,
};
use serde_json::Value;
use tempfile::TempDir;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pdfp"))
}

fn run_pdfp(args: &[String]) -> Output {
    Command::new(bin_path())
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run pdfp {args:?}: {err}"))
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

fn write_two_page_pdf(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });

    let mut kids = Vec::new();
    for page_num in 1..=2 {
        let content = format!("BT /F1 12 Tf 72 720 Td (Page {page_num}) Tj ET");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        kids.push(page_id.into());
    }

    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 2,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.save(path).unwrap();
}

fn add_signature_field(path: &Path) {
    let mut doc = Document::load(path).unwrap();
    let signature_id = doc.add_object(dictionary! {
        "FT" => "Sig",
    });
    let acro_form_id = doc.add_object(dictionary! {
        "Fields" => vec![signature_id.into()],
    });
    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_dictionary_mut(catalog_id)
        .unwrap()
        .set("AcroForm", acro_form_id);
    doc.save(path).unwrap();
}

fn encrypt_pdf(path: &Path) {
    let mut doc = Document::load(path).unwrap();
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(vec![3; 16], StringFormat::Literal),
            Object::String(vec![4; 16], StringFormat::Literal),
        ]),
    );
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner-secret",
        user_password: "user-secret",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    doc.save(path).unwrap();
}

fn page_object(doc: &Document, page_num: u32) -> &lopdf::Dictionary {
    let page_id = doc.get_pages()[&page_num];
    doc.get_object(page_id).unwrap().as_dict().unwrap()
}

fn object_number(object: &Object) -> f32 {
    match object {
        Object::Integer(value) => *value as f32,
        Object::Real(value) => *value,
        other => panic!("expected numeric object, got {other:?}"),
    }
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.0001,
        "expected {expected}, got {actual}"
    );
}

fn last_operation<'a>(operations: &'a [Operation], operator: &str) -> &'a Operation {
    operations
        .iter()
        .rfind(|operation| operation.operator == operator)
        .unwrap_or_else(|| panic!("missing {operator} operation"))
}

#[test]
fn pages_rotate_sets_rotation_on_selected_pages() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output = temp.path().join("rotated.pdf");
    write_two_page_pdf(&input);

    let result = run_pdfp(&[
        "pages".to_string(),
        "rotate".to_string(),
        path_arg(&input),
        "--pages".to_string(),
        "1".to_string(),
        "--degrees".to_string(),
        "90".to_string(),
        "-o".to_string(),
        path_arg(&output),
    ]);

    assert!(
        result.status.success(),
        "pages rotate failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let doc = Document::load(&output).unwrap();
    assert_eq!(
        page_object(&doc, 1).get(b"Rotate").unwrap(),
        &Object::Integer(90)
    );
    assert!(page_object(&doc, 2).get(b"Rotate").is_err());
}

#[test]
fn page_crop_sets_crop_box_on_selected_pages() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output = temp.path().join("cropped.pdf");
    write_two_page_pdf(&input);

    let result = run_pdfp(&[
        "page".to_string(),
        "crop".to_string(),
        path_arg(&input),
        "--pages".to_string(),
        "2".to_string(),
        "--box".to_string(),
        "10".to_string(),
        "20".to_string(),
        "300".to_string(),
        "400".to_string(),
        "-o".to_string(),
        path_arg(&output),
    ]);

    assert!(
        result.status.success(),
        "page crop failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let doc = Document::load(&output).unwrap();
    assert!(page_object(&doc, 1).get(b"CropBox").is_err());
    let crop_box = page_object(&doc, 2)
        .get(b"CropBox")
        .unwrap()
        .as_array()
        .unwrap();
    let values: Vec<f32> = crop_box.iter().map(object_number).collect();
    assert_eq!(values, vec![10.0, 20.0, 300.0, 400.0]);
}

#[test]
fn page_text_writes_searchable_styled_overlay_on_selected_page() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output = temp.path().join("text.pdf");
    write_two_page_pdf(&input);

    let result = run_pdfp(&[
        "page".to_string(),
        "text".to_string(),
        path_arg(&input),
        "--pages".to_string(),
        "2".to_string(),
        "--text".to_string(),
        "Approved €\nSecond line".to_string(),
        "--x".to_string(),
        "40".to_string(),
        "--y".to_string(),
        "60".to_string(),
        "--origin".to_string(),
        "top-left".to_string(),
        "--font".to_string(),
        "times-bold".to_string(),
        "--font-size".to_string(),
        "18".to_string(),
        "--line-height".to_string(),
        "24".to_string(),
        "--colour".to_string(),
        "#3366cc".to_string(),
        "-o".to_string(),
        path_arg(&output),
    ]);

    assert!(
        result.status.success(),
        "page text failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let doc = Document::load(&output).unwrap();
    let pages = doc.get_pages();
    assert_eq!(doc.get_page_contents(pages[&1]).len(), 1);
    assert_eq!(doc.get_page_contents(pages[&2]).len(), 2);

    let content = doc.get_and_decode_page_content(pages[&2]).unwrap();
    let rgb = last_operation(&content.operations, "rg");
    assert_close(object_number(&rgb.operands[0]), 0.2);
    assert_close(object_number(&rgb.operands[1]), 0.4);
    assert_close(object_number(&rgb.operands[2]), 0.8);

    let font = last_operation(&content.operations, "Tf");
    assert_close(object_number(&font.operands[1]), 18.0);
    let font_resource = font.operands[0].as_name().unwrap();
    let resources = page_object(&doc, 2)
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    let font_id = fonts.get(font_resource).unwrap().as_reference().unwrap();
    assert_eq!(
        doc.get_dictionary(font_id)
            .unwrap()
            .get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap(),
        b"Times-Bold"
    );

    let matrices: Vec<&Operation> = content
        .operations
        .iter()
        .filter(|operation| operation.operator == "Tm")
        .collect();
    assert_eq!(matrices.len(), 2);
    assert_close(object_number(&matrices[0].operands[4]), 40.0);
    assert_close(object_number(&matrices[0].operands[5]), 732.0);
    assert_close(object_number(&matrices[1].operands[5]), 708.0);

    let encoding = Encoding::SimpleEncoding(b"WinAnsiEncoding");
    let text_lines: Vec<String> = content
        .operations
        .iter()
        .filter(|operation| operation.operator == "Tj")
        .map(|operation| {
            Document::decode_text(&encoding, operation.operands[0].as_str().unwrap()).unwrap()
        })
        .collect();
    assert!(text_lines.ends_with(&["Approved €".to_string(), "Second line".to_string()]));

    let search = run_pdfp(&[
        "search".to_string(),
        path_arg(&output),
        "Approved".to_string(),
        "--ocr".to_string(),
        "off".to_string(),
        "--json".to_string(),
    ]);
    assert!(
        search.status.success(),
        "search failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&search.stdout),
        String::from_utf8_lossy(&search.stderr)
    );
    let report: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(report["matches"][0]["page"], 2);
}

#[test]
fn page_text_rejects_invalid_colour_without_writing_output() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output = temp.path().join("text.pdf");
    write_two_page_pdf(&input);

    let result = run_pdfp(&[
        "page".to_string(),
        "text".to_string(),
        path_arg(&input),
        "--text".to_string(),
        "Hello".to_string(),
        "--x".to_string(),
        "40".to_string(),
        "--y".to_string(),
        "60".to_string(),
        "--color".to_string(),
        "invisible-ish".to_string(),
        "-o".to_string(),
        path_arg(&output),
    ]);

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid --color"));
    assert!(!output.exists());
}

#[test]
fn page_text_requires_explicit_override_for_signed_pdf() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("signed.pdf");
    let refused_output = temp.path().join("refused.pdf");
    let forced_output = temp.path().join("forced.pdf");
    write_two_page_pdf(&input);
    add_signature_field(&input);

    let base_args = [
        "page".to_string(),
        "text".to_string(),
        path_arg(&input),
        "--text".to_string(),
        "Signed change".to_string(),
        "--x".to_string(),
        "40".to_string(),
        "--y".to_string(),
        "60".to_string(),
    ];
    let mut refused_args = base_args.to_vec();
    refused_args.extend(["-o".to_string(), path_arg(&refused_output)]);
    let refused = run_pdfp(&refused_args);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--force-signed"));
    assert!(!refused_output.exists());

    let mut forced_args = base_args.to_vec();
    forced_args.extend([
        "--force-signed".to_string(),
        "-o".to_string(),
        path_arg(&forced_output),
    ]);
    let forced = run_pdfp(&forced_args);
    assert!(
        forced.status.success(),
        "forced page text failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&forced.stdout),
        String::from_utf8_lossy(&forced.stderr)
    );
    assert!(String::from_utf8_lossy(&forced.stderr).contains("invalidate signatures"));
    let doc = Document::load(&forced_output).unwrap();
    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    assert!(doc.get_dictionary(catalog_id).unwrap().has(b"AcroForm"));
}

#[test]
fn page_text_refuses_encrypted_pdf_without_writing_output() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("encrypted.pdf");
    let output = temp.path().join("text.pdf");
    write_two_page_pdf(&input);
    encrypt_pdf(&input);

    let result = run_pdfp(&[
        "page".to_string(),
        "text".to_string(),
        path_arg(&input),
        "--text".to_string(),
        "No unsafe rewrite".to_string(),
        "--x".to_string(),
        "40".to_string(),
        "--y".to_string(),
        "60".to_string(),
        "-o".to_string(),
        path_arg(&output),
    ]);

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("encrypted/password-protected") && stderr.contains("qpdf --decrypt"),
        "stderr:\n{stderr}"
    );
    assert!(!output.exists());
}
