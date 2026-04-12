#![allow(dead_code)]

use std::path::PathBuf;

/// Bounding box using corner coordinates (matches mupdf::Rect convention).
/// Origin top-left, Y increases downward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bbox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Bbox {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    pub fn center_x(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }

    pub fn center_y(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    /// True if this bbox overlaps with other (touching edges count as overlap)
    pub fn overlaps(&self, other: &Bbox) -> bool {
        self.x0 < other.x1
            && self.x1 > other.x0
            && self.y0 < other.y1
            && self.y1 > other.y0
    }

    /// Vertical gap between bottom of self and top of other (negative = overlap)
    pub fn vertical_gap_to(&self, other: &Bbox) -> f32 {
        other.y0 - self.y1
    }

    /// Horizontal gap between right of self and left of other (negative = overlap)
    pub fn horizontal_gap_to(&self, other: &Bbox) -> f32 {
        other.x0 - self.x1
    }

    pub fn union(&self, other: &Bbox) -> Bbox {
        Bbox {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }
}

/// A single text block as extracted from mupdf, before layout analysis.
#[derive(Clone, Debug)]
pub struct RawTextBlock {
    pub bbox: Bbox,
    pub text: String,
    pub font_size: f32,
    pub font_name: String,
    pub page_num: usize,      // 0-indexed
    pub block_id: usize,      // stable index within page
    pub reading_order: usize, // assigned by XY-Cut; default 0
}

/// Reference to an image found on a page (via TextPage block iteration).
#[derive(Clone, Debug)]
pub struct ImageRef {
    pub page_num: usize,
    pub bbox: Bbox,
    pub image_index: usize, // index within page's image blocks
}

/// Block kind — determined by the classifier after layout analysis.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Heading { level: u8 }, // h1..h6
    Paragraph,
    ListItem { ordered: bool, depth: u8 },
    TableCell { row: usize, col: usize },
    Caption,
    CodeBlock,
    PageNumber,
    RunningHeader,
    RunningFooter,
    Image { path: Option<String> },
}

/// A classified, reading-order-assigned block.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: usize,
    pub bbox: Bbox,
    pub text: String,
    pub kind: BlockKind,
    pub font_size: f32,
    pub font_name: String,
    pub page_num: usize,
    pub reading_order: usize,
}

/// A raw page as extracted from mupdf (before layout analysis).
#[derive(Debug)]
pub struct RawPage {
    pub page_num: usize,
    pub width: f32,
    pub height: f32,
    pub blocks: Vec<RawTextBlock>,
    pub image_refs: Vec<ImageRef>,
}

impl RawPage {
    pub fn bbox(&self) -> Bbox {
        Bbox::new(0.0, 0.0, self.width, self.height)
    }
}

/// A page after layout analysis and classification.
#[derive(Debug)]
pub struct Page {
    pub page_num: usize,
    pub width: f32,
    pub height: f32,
    pub blocks: Vec<Block>, // sorted by reading_order
}

/// Document metadata from PDF info dictionary.
#[derive(Debug, Default, Clone)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub page_count: usize,
}

/// The full processed document.
#[derive(Debug)]
pub struct Document {
    pub source_path: PathBuf,
    pub pages: Vec<Page>,
    pub metadata: DocumentMetadata,
}

/// A section of a document (heading + its content blocks).
/// Produced by the markdown renderer; consumed by format modules.
#[derive(Debug, Clone)]
pub struct Section {
    pub title: String,
    pub level: u8,
    pub content: String, // markdown for this section only
    pub page_start: usize,
    pub page_end: usize,
}

/// An image that has been extracted to disk.
#[derive(Debug, Clone)]
pub struct ExtractedImage {
    pub page_num: usize,
    pub rel_path: String, // relative path from output dir (e.g. "images/fig1.png")
    pub abs_path: PathBuf,
}
