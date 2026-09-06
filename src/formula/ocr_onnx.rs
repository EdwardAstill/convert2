//! Native ONNX formula OCR via RapidLaTeX-OCR models.
//!
//! Enable with `cargo build --features onnx-ocr` and select at runtime with
//! `--formula-sidecar onnx:<model-dir>`.

use crate::formula::ocr::{FormulaSidecar, FormulaSidecarAttempt, FormulaSidecarStatus};
use anyhow::{bail, Context};
use image::imageops::{self, FilterType};
use ndarray::{Array2, Array4, ArrayD};
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;

const PAD_ID: i64 = 0;
const BOS_ID: i64 = 1;
const EOS_ID: i64 = 2;
const MAX_DECODE_STEPS: usize = 256;
const MODEL_HEIGHT: u32 = 192;
const MODEL_WIDTH: u32 = 672;

/// Formula OCR sidecar backed by local ONNX Runtime sessions.
pub struct OnnxFormulaSidecar {
    sessions: Option<OnnxSessions>,
    vocab: Vec<String>,
}

struct OnnxSessions {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
}

impl OnnxFormulaSidecar {
    pub fn new(model_dir: &Path) -> anyhow::Result<Self> {
        let encoder_path = model_dir.join("encoder.onnx");
        let decoder_path = model_dir.join("decoder.onnx");
        let vocab_path = model_dir.join("vocab.txt");

        if !encoder_path.exists() {
            bail!("encoder.onnx not found in {}", model_dir.display());
        }
        if !decoder_path.exists() {
            bail!("decoder.onnx not found in {}", model_dir.display());
        }
        if !vocab_path.exists() {
            bail!("vocab.txt not found in {}", model_dir.display());
        }

        let encoder = Session::builder()
            .context("failed to create ONNX encoder session builder")?
            .commit_from_file(&encoder_path)
            .with_context(|| format!("failed to load {}", encoder_path.display()))?;
        let decoder = Session::builder()
            .context("failed to create ONNX decoder session builder")?
            .commit_from_file(&decoder_path)
            .with_context(|| format!("failed to load {}", decoder_path.display()))?;
        let vocab = load_vocab(&vocab_path)?;

        Ok(Self {
            sessions: Some(OnnxSessions {
                encoder: Mutex::new(encoder),
                decoder: Mutex::new(decoder),
            }),
            vocab,
        })
    }

    #[doc(hidden)]
    pub fn from_parts_for_test(vocab: Vec<String>) -> Self {
        Self {
            sessions: None,
            vocab,
        }
    }

    fn recognize_inner(&self, crop: &Path) -> anyhow::Result<Option<String>> {
        let sessions = self
            .sessions
            .as_ref()
            .context("ONNX sessions are not loaded")?;
        let memory = self.encode_crop(sessions, crop)?;
        let token_ids = self.decode_tokens(sessions, memory)?;
        let latex = decode_ids(&token_ids, &self.vocab);
        if latex.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(latex))
        }
    }

    fn encode_crop(&self, sessions: &OnnxSessions, crop: &Path) -> anyhow::Result<ArrayD<f32>> {
        let input_tensor = preprocess_image(crop)?;
        let input = Tensor::from_array(input_tensor)?;
        let mut encoder = sessions
            .encoder
            .lock()
            .expect("ONNX encoder mutex poisoned");
        let outputs = encoder.run(ort::inputs![input])?;
        let memory = outputs[0]
            .try_extract_array::<f32>()
            .context("encoder output was not a float tensor")?
            .to_owned();
        Ok(memory)
    }

    fn decode_tokens(
        &self,
        sessions: &OnnxSessions,
        memory: ArrayD<f32>,
    ) -> anyhow::Result<Vec<i64>> {
        let mut token_ids = vec![BOS_ID];
        let mut decoder = sessions
            .decoder
            .lock()
            .expect("ONNX decoder mutex poisoned");

        // Pre-allocate context tensor once — it never changes per crop
        let context_tensor =
            Tensor::from_array(memory).context("failed to create context tensor")?;

        for _ in 0..MAX_DECODE_STEPS {
            let seq_len = token_ids.len();
            let ids_array = Array2::from_shape_vec((1, seq_len), token_ids.clone())
                .context("failed to build decoder token tensor")?;
            let ids = Tensor::from_array(ids_array)?;
            let mask_array = Array2::<bool>::from_elem((seq_len, seq_len), false);
            let mask = Tensor::from_array(mask_array)?;
            let outputs = decoder
                .run(ort::inputs!["x" => ids, "mask" => mask, "context" => &context_tensor])?;
            let logits = outputs[0]
                .try_extract_array::<f32>()
                .context("decoder output was not a float tensor")?;
            let next_id = argmax_last_step(&logits)?;

            token_ids.push(next_id);
            if next_id == EOS_ID {
                break;
            }
        }

        Ok(token_ids)
    }
}

impl FormulaSidecar for OnnxFormulaSidecar {
    fn recognize(&self, crop_path: &Path) -> FormulaSidecarAttempt {
        let start = std::time::Instant::now();
        match self.recognize_inner(crop_path) {
            Ok(Some(latex)) => FormulaSidecarAttempt {
                status: FormulaSidecarStatus::Recovered,
                backend: Some("onnx-formula-sidecar".to_string()),
                latex: Some(latex),
                duration_ms: Some(elapsed_ms(start)),
                stderr: None,
                error: None,
                sanity: None,
            },
            Ok(None) => FormulaSidecarAttempt {
                status: FormulaSidecarStatus::EmptyOutput,
                backend: Some("onnx-formula-sidecar".to_string()),
                latex: None,
                duration_ms: Some(elapsed_ms(start)),
                stderr: None,
                error: None,
                sanity: None,
            },
            Err(err) => FormulaSidecarAttempt {
                status: FormulaSidecarStatus::CommandFailed,
                backend: Some("onnx-formula-sidecar".to_string()),
                latex: None,
                duration_ms: Some(elapsed_ms(start)),
                stderr: None,
                error: Some(err.to_string()),
                sanity: None,
            },
        }
    }
}

fn elapsed_ms(start: std::time::Instant) -> u64 {
    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// Load a formula crop image, resize to `[1, 1, 192, 672]`, and normalise to `[0, 1]`.
pub fn preprocess_image(path: &Path) -> anyhow::Result<Array4<f32>> {
    let mut image = image::open(path)
        .with_context(|| format!("failed to open formula crop {}", path.display()))?
        .to_luma8();
    imageops::colorops::invert(&mut image);
    let resized = imageops::resize(&image, MODEL_WIDTH, MODEL_HEIGHT, FilterType::Lanczos3);

    let mut tensor = Array4::<f32>::zeros((1, 1, MODEL_HEIGHT as usize, MODEL_WIDTH as usize));
    for (y, row) in resized.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            tensor[[0, 0, y, x]] = f32::from(pixel[0]) / 255.0;
        }
    }

    Ok(tensor)
}

/// Load vocabulary: one token per line, index = line number.
pub fn load_vocab(path: &Path) -> anyhow::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to load vocab {}", path.display()))?;
    Ok(content.lines().map(str::to_string).collect())
}

/// Decode token IDs to a LaTeX string, skipping BOS/EOS/PAD and stopping at EOS.
///
/// BPE tokenizers split LaTeX commands into backslash + name tokens (e.g.
/// `\` + `mathrm`). This function merges `\ ` + letter sequences so that
/// `\ mathrm` becomes `\mathrm`.
pub fn decode_ids(ids: &[i64], vocab: &[String]) -> String {
    let mut tokens = Vec::new();
    for &id in ids {
        if id == EOS_ID {
            break;
        }
        if id == BOS_ID || id == PAD_ID || id.is_negative() {
            continue;
        }
        if let Some(token) = vocab.get(id as usize) {
            tokens.push(token.as_str());
        }
    }
    merge_bpe_backslash(tokens.join(" "))
}

/// Merge BPE `\ ` + letter pairs so that `\ mathrm` → `\mathrm`.
fn merge_bpe_backslash(raw: String) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] == '\\'
            && i + 1 < n
            && chars[i + 1] == ' '
            && i + 2 < n
            && chars[i + 2].is_ascii_alphabetic()
        {
            out.push('\\');
            i += 2; // skip the space
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn argmax_last_step(logits: &ndarray::ArrayViewD<'_, f32>) -> anyhow::Result<i64> {
    let shape = logits.shape();
    if shape.len() < 2 {
        bail!("decoder logits must have at least 2 dimensions");
    }
    let vocab_size = *shape.last().expect("shape has at least 2 dimensions");
    let step_count = shape[shape.len() - 2];
    if vocab_size == 0 || step_count == 0 {
        bail!("decoder logits have empty sequence or vocab dimension");
    }
    let offset = (step_count - 1) * vocab_size;
    let values = logits
        .as_slice()
        .context("decoder logits are not contiguous")?
        .get(offset..offset + vocab_size)
        .context("decoder logits shape does not match storage")?;
    let next_id = values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(idx, _)| idx as i64)
        .context("decoder logits are empty")?;
    Ok(next_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_bpe_preserves_plain_text() {
        assert_eq!(merge_bpe_backslash("hello world".into()), "hello world");
    }

    #[test]
    fn merge_bpe_fixes_backslash_command() {
        assert_eq!(
            merge_bpe_backslash("\\ mathrm {abc}".into()),
            "\\mathrm {abc}"
        );
    }

    #[test]
    fn merge_bpe_fixes_multiple_commands() {
        assert_eq!(
            merge_bpe_backslash("\\ frac {a}{b} + \\ sqrt {x}".into()),
            "\\frac {a}{b} + \\sqrt {x}"
        );
    }

    #[test]
    fn merge_bpe_leaves_standalone_backslash_space() {
        assert_eq!(merge_bpe_backslash("x \\ 1".into()), "x \\ 1");
    }

    #[test]
    fn merge_bpe_empty_string() {
        assert_eq!(merge_bpe_backslash(String::new()), "");
    }

    #[test]
    fn decode_ids_joins_and_merges() {
        let vocab = ["[PAD]", "[BOS]", "[EOS]", "x", "\\", "frac", "a"]
            .map(str::to_string)
            .to_vec();
        let ids: Vec<i64> = vec![1, 3, 4, 5, 3, 6, 2];
        let result = decode_ids(&ids, &vocab);
        assert_eq!(result, "x \\frac x a");
    }
}
