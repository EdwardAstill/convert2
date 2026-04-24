use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use regex::Regex;
use std::sync::OnceLock;
use serde::{Deserialize, Serialize};
use crate::document::types::Document;
use crate::error::{VtvError, VtvResult};
use crate::render::markdown::RenderedDocument;

// --- Regex helpers ---

fn citation_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[(?:[A-Z][a-z]+(?:,\s*[A-Z][a-z]+)*(?:,?\s*(?:19|20)\d{2})|[\d,\s]+)\]")
            .unwrap()
    })
}

fn concept_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches multi-word capitalized phrases (2-4 words) — a good concept heuristic
    RE.get_or_init(|| {
        Regex::new(r"\b([A-Z][a-zA-Z]+(?:\s+[A-Z][a-zA-Z]+){1,3})\b").unwrap()
    })
}

// --- Output types ---

#[derive(Serialize, Deserialize, Debug)]
pub struct KnowledgeGraph {
    pub metadata: GraphMetadata,
    pub nodes: Vec<KgNode>,
    pub edges: Vec<KgEdge>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GraphMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub source: String,
    pub section_count: usize,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KgNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum NodeKind {
    Section,
    Concept,
    Citation,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KgEdge {
    pub source: String,
    pub target: String,
    pub relation: EdgeRelation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum EdgeRelation {
    Contains,
    Cites,
    RelatedTo,
}

pub struct KgFormat;

impl KgFormat {
    pub fn write(
        rendered: &RenderedDocument,
        doc: &Document,
        output_dir: &Path,
        stem: &str,
    ) -> VtvResult<()> {
        fs::create_dir_all(output_dir).map_err(|e| VtvError::Io {
            path: output_dir.to_path_buf(),
            source: e,
        })?;

        let graph = build_graph(rendered, doc);

        let json = serde_json::to_string_pretty(&graph).map_err(VtvError::Serialization)?;

        let path = output_dir.join(format!("{}_graph.json", stem));
        fs::write(&path, &json).map_err(|e| VtvError::Io {
            path: path.clone(),
            source: e,
        })?;

        println!(
            "  wrote graph: {} nodes, {} edges → {}",
            graph.nodes.len(),
            graph.edges.len(),
            path.display()
        );
        Ok(())
    }
}

fn build_graph(rendered: &RenderedDocument, doc: &Document) -> KnowledgeGraph {
    let mut nodes: Vec<KgNode> = Vec::new();
    let mut edges: Vec<KgEdge> = Vec::new();

    // --- 1. Section nodes ---
    for section in &rendered.sections {
        let id = section_id(&section.title);
        nodes.push(KgNode {
            id,
            label: section.title.clone(),
            kind: NodeKind::Section,
            page: Some(section.page_start),
            frequency: None,
            excerpt: Some(section.content.chars().take(120).collect()),
        });
    }

    // --- 2. Extract concepts and citations per section ---
    // concept_label → frequency across all sections
    let mut concept_freq: HashMap<String, usize> = HashMap::new();
    // citation_label → set of section ids that cite it
    let mut citation_sections: HashMap<String, HashSet<String>> = HashMap::new();
    // section_id → set of concept labels found in it
    let mut section_concepts: HashMap<String, HashSet<String>> = HashMap::new();

    for section in &rendered.sections {
        let sec_id = section_id(&section.title);
        let text = &section.content;

        // Extract concepts
        let concepts: HashSet<String> = concept_re()
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect();

        for concept in &concepts {
            *concept_freq.entry(concept.clone()).or_insert(0) += 1;
        }
        section_concepts.insert(sec_id.clone(), concepts);

        // Extract citations
        let citations: HashSet<String> = citation_re()
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .collect();

        for citation in citations {
            citation_sections
                .entry(citation)
                .or_default()
                .insert(sec_id.clone());
        }
    }

    // --- 3. Concept nodes (min frequency 2 to filter noise) ---
    // Also exclude concepts that are section titles (they're already Section nodes)
    let section_titles: HashSet<String> = rendered.sections.iter().map(|s| s.title.clone()).collect();

    for (concept, freq) in &concept_freq {
        if *freq >= 2 && !section_titles.contains(concept) && concept.len() > 3 {
            nodes.push(KgNode {
                id: concept_id(concept),
                label: concept.clone(),
                kind: NodeKind::Concept,
                page: None,
                frequency: Some(*freq),
                excerpt: None,
            });
        }
    }

    // --- 4. Citation nodes ---
    for citation in citation_sections.keys() {
        nodes.push(KgNode {
            id: citation_id(citation),
            label: citation.clone(),
            kind: NodeKind::Citation,
            page: None,
            frequency: None,
            excerpt: None,
        });
    }

    // --- 5. Contains edges: section → concept ---
    let concept_node_ids: HashSet<String> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Concept)
        .map(|n| n.id.clone())
        .collect();

    for section in &rendered.sections {
        let sec_id = section_id(&section.title);
        if let Some(concepts) = section_concepts.get(&sec_id) {
            for concept in concepts {
                let cid = concept_id(concept);
                if concept_node_ids.contains(&cid) {
                    let freq = concept_freq.get(concept).copied().unwrap_or(1);
                    let max_freq = concept_freq.values().copied().max().unwrap_or(1);
                    let weight = freq as f32 / max_freq as f32;
                    edges.push(KgEdge {
                        source: sec_id.clone(),
                        target: cid,
                        relation: EdgeRelation::Contains,
                        weight: Some(weight),
                    });
                }
            }
        }
    }

    // --- 6. Cites edges: section → citation ---
    for (citation, sec_ids) in &citation_sections {
        let cit_id = citation_id(citation);
        for sec_id in sec_ids {
            edges.push(KgEdge {
                source: sec_id.clone(),
                target: cit_id.clone(),
                relation: EdgeRelation::Cites,
                weight: None,
            });
        }
    }

    // --- 7. RelatedTo edges: section → section (via shared concepts, min 2) ---
    let section_ids: Vec<String> = rendered.sections.iter().map(|s| section_id(&s.title)).collect();

    for i in 0..section_ids.len() {
        for j in (i + 1)..section_ids.len() {
            let a = &section_ids[i];
            let b = &section_ids[j];
            let empty = HashSet::new();
            let concepts_a = section_concepts.get(a).unwrap_or(&empty);
            let concepts_b = section_concepts.get(b).unwrap_or(&empty);
            let shared: HashSet<_> = concepts_a.intersection(concepts_b).collect();
            if shared.len() >= 2 {
                let weight = shared.len() as f32
                    / concepts_a.len().max(concepts_b.len()).max(1) as f32;
                edges.push(KgEdge {
                    source: a.clone(),
                    target: b.clone(),
                    relation: EdgeRelation::RelatedTo,
                    weight: Some(weight),
                });
            }
        }
    }

    let node_count = nodes.len();
    let edge_count = edges.len();

    KnowledgeGraph {
        metadata: GraphMetadata {
            title: doc.metadata.title.clone(),
            author: doc.metadata.author.clone(),
            source: doc
                .source_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            section_count: rendered.sections.len(),
            node_count,
            edge_count,
        },
        nodes,
        edges,
    }
}

fn section_id(title: &str) -> String {
    format!("sec_{}", slugify(title))
}

fn concept_id(concept: &str) -> String {
    format!("concept_{}", slugify(concept))
}

fn citation_id(citation: &str) -> String {
    format!("cite_{}", slugify(citation))
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}
