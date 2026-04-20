//! HTTP client for [`docling-serve`](https://github.com/docling-project/docling-serve).
//!
//! Posts a PDF to `POST /v1/convert/file` and extracts the `md_content` field
//! from the `DoclingDocument` response. Uses `reqwest::blocking` — no tokio.

use std::time::Duration;

use reqwest::blocking::{multipart, Client};
use serde::Deserialize;

use crate::error::{VtvError, VtvResult};

/// Request options we send to docling-serve. Mirrors the subset of
/// `PipelineOptions` that affects output quality for our use case.
const DOCLING_OPTIONS: &[(&str, &str)] = &[
    ("to_formats", "md"),
    ("do_ocr", "true"),
    ("do_table_structure", "true"),
    ("do_formula_enrichment", "true"),
    ("do_code_enrichment", "false"),
    ("do_picture_description", "false"),
];

/// Blocking HTTP client for docling-serve.
pub struct DoclingClient {
    base_url: String,
    http: Client,
}

impl DoclingClient {
    pub fn new(base_url: &str, timeout: Duration) -> Self {
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest blocking client builds");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// Upload in-memory PDF bytes to docling-serve and return the response's
    /// `md_content`. Used by per-page routing where the bytes come from a
    /// single-page PDF extracted in memory.
    pub fn convert_bytes_to_markdown(
        &self,
        bytes: Vec<u8>,
        filename: &str,
    ) -> VtvResult<String> {
        let endpoint = format!("{}/v1/convert/file", self.base_url);
        let part = multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str("application/pdf")
            .map_err(|e| VtvError::HybridBackend {
                url: endpoint.clone(),
                message: format!("could not build multipart part: {e}"),
            })?;
        let mut form = multipart::Form::new().part("files", part);
        for (key, value) in DOCLING_OPTIONS {
            form = form.text(*key, *value);
        }

        self.send_and_parse(&endpoint, form)
    }

    fn send_and_parse(&self, endpoint: &str, form: multipart::Form) -> VtvResult<String> {
        let response = self
            .http
            .post(endpoint)
            .multipart(form)
            .send()
            .map_err(|e| VtvError::HybridBackend {
                url: endpoint.to_string(),
                message: format!("request failed: {e}"),
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(VtvError::HybridBackend {
                url: endpoint.to_string(),
                message: format!("HTTP {status}: {body}"),
            });
        }

        let parsed: ConvertResponse =
            response.json().map_err(|e| VtvError::HybridBackend {
                url: endpoint.to_string(),
                message: format!("response was not valid JSON: {e}"),
            })?;

        let md = parsed.extract_markdown().ok_or_else(|| VtvError::HybridBackend {
            url: endpoint.to_string(),
            message: "response contained no markdown content".to_string(),
        })?;
        if md.trim().is_empty() {
            return Err(VtvError::HybridBackend {
                url: self.base_url.clone(),
                message: "docling returned empty markdown".to_string(),
            });
        }
        Ok(md)
    }
}

/// Minimal view of docling-serve's `ConvertDocumentResponse`. The schema has
/// moved around between versions — we accept markdown at several plausible
/// keys and take the first non-empty one.
#[derive(Debug, Deserialize)]
struct ConvertResponse {
    #[serde(default)]
    document: Option<DoclingDocumentField>,
    #[serde(default)]
    md_content: Option<String>,
    #[serde(default)]
    content_md: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DoclingDocumentField {
    #[serde(default)]
    md_content: Option<String>,
    #[serde(default)]
    content_md: Option<String>,
}

impl ConvertResponse {
    fn extract_markdown(self) -> Option<String> {
        if let Some(md) = self.md_content.filter(|s| !s.is_empty()) {
            return Some(md);
        }
        if let Some(md) = self.content_md.filter(|s| !s.is_empty()) {
            return Some(md);
        }
        if let Some(doc) = self.document {
            if let Some(md) = doc.md_content.filter(|s| !s.is_empty()) {
                return Some(md);
            }
            if let Some(md) = doc.content_md.filter(|s| !s.is_empty()) {
                return Some(md);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_markdown_from_top_level_md_content() {
        let json = r##"{"md_content": "# Hello"}"##;
        let parsed: ConvertResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.extract_markdown().as_deref(), Some("# Hello"));
    }

    #[test]
    fn extract_markdown_from_document_md_content() {
        let json = r##"{"document": {"md_content": "# Hi"}}"##;
        let parsed: ConvertResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.extract_markdown().as_deref(), Some("# Hi"));
    }

    #[test]
    fn extract_markdown_none_when_all_empty() {
        let json = r##"{"md_content": "", "document": {"md_content": ""}}"##;
        let parsed: ConvertResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.extract_markdown().is_none());
    }

    #[test]
    fn extract_markdown_none_when_missing() {
        let json = r##"{}"##;
        let parsed: ConvertResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.extract_markdown().is_none());
    }

    #[test]
    fn extract_markdown_prefers_top_level_md_content() {
        let json = r##"{"md_content": "top", "document": {"md_content": "nested"}}"##;
        let parsed: ConvertResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.extract_markdown().as_deref(), Some("top"));
    }
}
