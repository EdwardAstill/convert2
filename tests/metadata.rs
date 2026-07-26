//! CLI integration tests for PDF document information metadata.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{
    dictionary, text_string, Document, EncryptionState, EncryptionVersion, Object, Permissions,
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

fn write_pdf(path: &Path, info: &[(&str, &str)], xmp: bool, signed: bool) {
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
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (Hello metadata) Tj ET".to_vec(),
    ));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Resources" => resources_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );

    let mut catalog = dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    };
    if xmp {
        let xmp_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "Metadata",
                "Subtype" => "XML",
            },
            b"<x:xmpmeta></x:xmpmeta>".to_vec(),
        ));
        catalog.set("Metadata", xmp_id);
    }
    if signed {
        let sig_id = doc.add_object(dictionary! {
            "FT" => "Sig",
            "T" => text_string("Signature1"),
        });
        let acro_form_id = doc.add_object(dictionary! {
            "Fields" => vec![sig_id.into()],
        });
        catalog.set("AcroForm", acro_form_id);
    }

    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);

    if !info.is_empty() {
        let mut info_dict = lopdf::Dictionary::new();
        for (key, value) in info {
            info_dict.set(*key, text_string(value));
        }
        let info_id = doc.add_object(info_dict);
        doc.trailer.set("Info", info_id);
    }

    doc.save(path).unwrap();
}

fn write_encrypted_pdf(path: &Path, user_password: &str) {
    write_pdf(path, &[("Title", "Encrypted")], false, false);

    let mut doc = Document::load(path).unwrap();
    doc.trailer.set(
        "ID",
        Object::Array(vec![
            Object::String(vec![1; 16], StringFormat::Literal),
            Object::String(vec![2; 16], StringFormat::Literal),
        ]),
    );
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner-secret",
        user_password,
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    doc.save(path).unwrap();
}

fn show_json(path: &Path) -> Value {
    let output = run_pdfp(&[
        "metadata".to_string(),
        "show".to_string(),
        path_arg(path),
        "--json".to_string(),
    ]);
    assert!(
        output.status.success(),
        "metadata show failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn metadata_show_json_reports_full_info() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    write_pdf(
        &input,
        &[
            ("Title", "Original Title"),
            ("Author", "Ada"),
            ("Subject", "Metadata"),
            ("Keywords", "pdf,rust"),
            ("Creator", "fixture"),
            ("Producer", "lopdf"),
            ("CreationDate", "D:20260102030405Z"),
            ("ModDate", "D:20260103040506Z"),
        ],
        false,
        false,
    );

    let json = show_json(&input);
    assert_eq!(json["page_count"], 1);
    assert_eq!(json["info"]["title"], "Original Title");
    assert_eq!(json["info"]["author"], "Ada");
    assert_eq!(json["info"]["subject"], "Metadata");
    assert_eq!(json["info"]["keywords"], "pdf,rust");
    assert_eq!(json["info"]["creator"], "fixture");
    assert_eq!(json["info"]["producer"], "lopdf");
    assert_eq!(json["info"]["creation_date"], "D:20260102030405Z");
    assert_eq!(json["info"]["modification_date"], "D:20260103040506Z");
    assert_eq!(json["xmp"]["present"], false);
    assert_eq!(json["signatures"]["present"], false);
}

#[test]
fn metadata_set_round_trips_selected_fields() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("output.pdf");
    write_pdf(&input, &[("Title", "Original")], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
        "--author".to_string(),
        "Grace Hopper".to_string(),
        "--subject".to_string(),
        "Metadata audit".to_string(),
        "--keywords".to_string(),
        "pdf,metadata".to_string(),
        "--creator".to_string(),
        "pdfp test".to_string(),
        "--producer".to_string(),
        "pdfp".to_string(),
        "--creation-date".to_string(),
        "2026-05-19T12:30:00Z".to_string(),
        "--mod-date".to_string(),
        "2026-05-20T01:02:03+08:00".to_string(),
        "--json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["changed"]
        .as_array()
        .unwrap()
        .contains(&Value::from("title")));

    let json = show_json(&output_path);
    assert_eq!(json["info"]["title"], "Updated");
    assert_eq!(json["info"]["author"], "Grace Hopper");
    assert_eq!(json["info"]["subject"], "Metadata audit");
    assert_eq!(json["info"]["keywords"], "pdf,metadata");
    assert_eq!(json["info"]["creator"], "pdfp test");
    assert_eq!(json["info"]["producer"], "pdfp");
    assert_eq!(json["info"]["creation_date"], "D:20260519123000Z");
    assert_eq!(json["info"]["modification_date"], "D:20260520010203+08'00'");
}

#[test]
fn metadata_set_preserves_unicode_title() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("unicode.pdf");
    write_pdf(&input, &[], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Resume Ω".to_string(),
        "--no-touch-mod-date".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json = show_json(&output_path);
    assert_eq!(json["info"]["title"], "Resume Ω");
}

#[test]
fn metadata_set_preserves_an_empty_present_value() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("empty-title.pdf");
    write_pdf(&input, &[("Title", "Original")], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        String::new(),
        "--no-touch-mod-date".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json = show_json(&output_path);
    assert_eq!(json["info"]["title"], "");
}

#[test]
fn metadata_set_updates_modification_date_by_default() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("dated.pdf");
    write_pdf(&input, &[], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mod_date = show_json(&output_path)["info"]["modification_date"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(mod_date.starts_with("D:"), "modification date: {mod_date}");
    assert!(mod_date.ends_with('Z'), "modification date: {mod_date}");
}

#[test]
fn metadata_clear_removes_selected_fields() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("cleared.pdf");
    write_pdf(
        &input,
        &[
            ("Title", "Original"),
            ("Author", "Ada"),
            ("Subject", "Keep"),
        ],
        false,
        false,
    );

    let output = run_pdfp(&[
        "metadata".to_string(),
        "clear".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--fields".to_string(),
        "title,author".to_string(),
        "--json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata clear failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json = show_json(&output_path);
    assert_eq!(json["info"]["title"], Value::Null);
    assert_eq!(json["info"]["author"], Value::Null);
    assert_eq!(json["info"]["subject"], "Keep");
}

#[test]
fn metadata_clear_all_removes_every_supported_field() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("cleared.pdf");
    write_pdf(
        &input,
        &[
            ("Title", "Original"),
            ("Author", "Ada"),
            ("Subject", "Metadata"),
            ("Keywords", "pdf,rust"),
            ("Creator", "fixture"),
            ("Producer", "lopdf"),
            ("CreationDate", "D:20260102030405Z"),
            ("ModDate", "D:20260103040506Z"),
        ],
        false,
        false,
    );

    let output = run_pdfp(&[
        "metadata".to_string(),
        "clear".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--fields".to_string(),
        "all".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata clear failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let info = show_json(&output_path)["info"].as_object().unwrap().clone();
    assert!(
        info.values().all(Value::is_null),
        "metadata remained: {info:?}"
    );
}

#[test]
fn metadata_refuses_same_input_output() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    write_pdf(&input, &[], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&input),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to overwrite input PDF"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn metadata_refuses_equivalent_same_input_output() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let equivalent_output = temp.path().join(".").join("input.pdf");
    write_pdf(&input, &[], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&equivalent_output),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to overwrite input PDF"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn metadata_warns_when_xmp_present() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("output.pdf");
    write_pdf(&input, &[], true, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
        "--no-touch-mod-date".to_string(),
        "--json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let warnings = report["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .unwrap()
            .contains("XMP metadata is present")),
        "{warnings:?}"
    );
    assert_eq!(show_json(&output_path)["xmp"]["present"], true);
}

#[test]
fn metadata_refuses_signed_pdf_without_force() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("signed.pdf");
    let output_path = temp.path().join("output.pdf");
    write_pdf(&input, &[], false, true);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("signature fields") && stderr.contains("--force-signed"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn metadata_force_signed_writes_with_a_warning() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("signed.pdf");
    let output_path = temp.path().join("output.pdf");
    write_pdf(&input, &[], false, true);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
        "--no-touch-mod-date".to_string(),
        "--force-signed".to_string(),
        "--json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "metadata set failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str().unwrap().contains("signature fields")));
    let output_metadata = show_json(&output_path);
    assert_eq!(output_metadata["info"]["title"], "Updated");
    assert_eq!(output_metadata["signatures"]["present"], true);
}

#[test]
fn metadata_refuses_encrypted_pdf_without_creating_output() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("encrypted.pdf");
    let output_path = temp.path().join("output.pdf");
    write_encrypted_pdf(&input, "user-secret");

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("encrypted/password-protected") && stderr.contains("qpdf --decrypt"),
        "stderr:\n{stderr}"
    );
    assert!(!output_path.exists());
}

#[test]
fn metadata_refuses_empty_password_encryption_without_stripping_it() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("encrypted.pdf");
    let output_path = temp.path().join("output.pdf");
    write_encrypted_pdf(&input, "");

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--title".to_string(),
        "Updated".to_string(),
    ]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("encrypted/password-protected"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output_path.exists());
}

#[test]
fn metadata_rejects_invalid_pdf_date() {
    let temp = TempDir::new().unwrap();
    let input = temp.path().join("input.pdf");
    let output_path = temp.path().join("output.pdf");
    write_pdf(&input, &[], false, false);

    let output = run_pdfp(&[
        "metadata".to_string(),
        "set".to_string(),
        path_arg(&input),
        "-o".to_string(),
        path_arg(&output_path),
        "--creation-date".to_string(),
        "D:20261319123000Z".to_string(),
    ]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("valid PDF date"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
