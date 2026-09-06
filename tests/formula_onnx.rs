#[cfg(feature = "onnx-ocr")]
mod onnx_tests {
    use pdf_processor::formula::ocr::{FormulaSidecar, FormulaSidecarStatus};
    use pdf_processor::formula::ocr_onnx::{
        decode_ids, load_vocab, preprocess_image, OnnxFormulaSidecar,
    };
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn onnx_ocr_module_accessible() {
        let _ = std::marker::PhantomData::<OnnxFormulaSidecar>;
    }

    #[test]
    fn preprocess_returns_correct_shape() {
        let png_path =
            std::env::temp_dir().join(format!("pdfp-test-formula-crop-{}.png", std::process::id()));
        image::GrayImage::from_pixel(20, 10, image::Luma([255]))
            .save(&png_path)
            .expect("write test PNG");

        let tensor = preprocess_image(&png_path).expect("preprocess");

        assert_eq!(tensor.shape(), &[1, 1, 192, 672]);
        assert!(tensor.iter().all(|&v| (0.0..=1.0).contains(&v)));

        let _ = std::fs::remove_file(png_path);
    }

    #[test]
    fn load_vocab_reads_lines_as_tokens() {
        let file = write_vocab_file(&["<PAD>", "<BOS>", "<EOS>", "<UNK>", "\\frac", "x", "+"]);

        let vocab = load_vocab(file.path()).expect("load vocab");

        assert_eq!(vocab.len(), 7);
        assert_eq!(vocab[4], "\\frac");
    }

    #[test]
    fn decode_ids_joins_tokens_and_strips_special() {
        let vocab = vec![
            "<PAD>".to_string(),
            "<BOS>".to_string(),
            "<EOS>".to_string(),
            "<UNK>".to_string(),
            "\\frac".to_string(),
            "{x}".to_string(),
            "{y}".to_string(),
        ];

        let result = decode_ids(&[1, 4, 5, 6, 2], &vocab);

        assert_eq!(result, "\\frac {x} {y}");
    }

    #[test]
    fn decode_ids_stops_at_eos() {
        let vocab = vec![
            "<PAD>".to_string(),
            "<BOS>".to_string(),
            "<EOS>".to_string(),
            "<UNK>".to_string(),
            "a".to_string(),
            "b".to_string(),
        ];

        let result = decode_ids(&[1, 4, 2, 5], &vocab);

        assert_eq!(result, "a");
    }

    #[test]
    fn new_with_missing_dir_returns_error() {
        let result = OnnxFormulaSidecar::new(Path::new("/nonexistent/pdfp-model-dir"));

        assert!(result.is_err(), "missing model dir must return Err");
    }

    #[test]
    fn new_with_incomplete_model_dir_returns_error() {
        let dir = tempfile::tempdir().expect("temp model dir");

        let result = OnnxFormulaSidecar::new(dir.path());

        assert!(result.is_err(), "incomplete model dir must return Err");
    }

    #[test]
    fn recognize_returns_none_when_crop_is_missing() {
        let sidecar = OnnxFormulaSidecar::from_parts_for_test(vec!["<PAD>".into()]);

        assert_eq!(
            sidecar.recognize(Path::new("/nonexistent/crop.png")).status,
            FormulaSidecarStatus::CommandFailed
        );
    }

    fn write_vocab_file(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp vocab");
        for line in lines {
            writeln!(file, "{line}").expect("write vocab line");
        }
        file
    }
}

#[cfg(feature = "onnx-ocr")]
mod cli_tests {
    use pdf_processor::config::{parse_formula_sidecar, FormulaSidecarArg};

    #[test]
    fn onnx_prefix_parses_to_model_dir() {
        let result = parse_formula_sidecar("onnx:/tmp/models").expect("parse");

        assert!(matches!(result, FormulaSidecarArg::Onnx(_)));
    }

    #[test]
    fn cmd_prefix_parses_to_command() {
        let result = parse_formula_sidecar("cmd:rapid-latex-ocr").expect("parse");

        assert!(matches!(result, FormulaSidecarArg::Command(_)));
    }

    #[test]
    fn bare_sidecar_value_remains_a_command() {
        let result = parse_formula_sidecar("rapid-latex-ocr").expect("parse");

        assert!(
            matches!(result, FormulaSidecarArg::Command(command) if command == "rapid-latex-ocr")
        );
    }
}
