//! PDF export for the Studio. This is the only place fonts and the PDF writer
//! enter the server; the browser never sees them.

use std::collections::BTreeMap;
use std::path::Path;

use soma_layout::{ImageSource, TextSource};
use soma_render_pdf::{render, PdfFont, PdfFonts, PdfMetadata};
use soma_studio_core::Build;

use crate::FontEntry;

pub struct FileImages {
    files: BTreeMap<String, Vec<u8>>,
}

impl FileImages {
    pub fn from_build(root: &Path, build: &Build) -> Self {
        let mut files = BTreeMap::new();
        for page in &build.pages.pages {
            for block in &page.blocks {
                if block.kind != soma_layout::BoxKind::Figure {
                    continue;
                }
                let Some(src) = build.pages.text.block_image(block.node) else {
                    continue;
                };
                if files.contains_key(src) || src.contains("://") || src.starts_with("data:") {
                    continue;
                }
                if let Some(path) = crate::safe_join(root, src) {
                    if let Ok(bytes) = std::fs::read(path) {
                        files.insert(String::from(src), bytes);
                    }
                }
            }
        }
        for media in &build.media {
            files
                .entry(media.name.clone())
                .or_insert_with(|| media.bytes.clone());
        }
        Self { files }
    }
}

impl ImageSource for FileImages {
    fn image(&self, src: &str) -> Option<&[u8]> {
        self.files.get(src).map(Vec::as_slice)
    }
}

pub fn render_pdf(
    build: &Build,
    fonts: &BTreeMap<String, FontEntry>,
    root: &Path,
) -> Result<Vec<u8>, String> {
    let regular = font(fonts, "serif")?.ok_or_else(|| {
        String::from("服务端没有配置 PDF 正文字体；用 --font <file> 启动 soma-studio")
    })?;
    let bold = font(fonts, "serif-bold")?;
    let sans = font(fonts, "sans")?;
    let sans_bold = font(fonts, "sans-bold")?;
    let mono = font(fonts, "mono")?;
    let mono_bold = font(fonts, "mono-bold")?;
    let math = font(fonts, "math")?;
    let fallback = font(fonts, "fallback")?;

    let fonts = PdfFonts {
        regular: &regular,
        bold: bold.as_ref(),
        sans: sans.as_ref(),
        sans_bold: sans_bold.as_ref(),
        mono: mono.as_ref(),
        mono_bold: mono_bold.as_ref(),
        math: math.as_ref(),
        fallback: fallback.as_ref(),
    };
    let author = {
        let names: Vec<&str> = build
            .document
            .meta
            .authors
            .iter()
            .map(|author| author.name.as_str())
            .collect();
        if names.is_empty() {
            None
        } else {
            Some(names.join(", "))
        }
    };
    let title = build.document.title_text();
    let metadata = PdfMetadata {
        title: title.as_deref(),
        author: author.as_deref(),
        language: build.document.meta.language.as_deref(),
    };
    let images = FileImages::from_build(root, build);
    render(
        &build.pages,
        &build.styled,
        &build.pages.text,
        &fonts,
        &metadata,
        &images,
    )
    .map_err(|error| format!("pdf: {error}"))
}

fn font(fonts: &BTreeMap<String, FontEntry>, role: &str) -> Result<Option<PdfFont>, String> {
    let Some(entry) = fonts.get(role) else {
        return Ok(None);
    };
    PdfFont::from_bytes(entry.bytes.clone(), entry.index)
        .map(Some)
        .map_err(|error| format!("{}: {error}", entry.info.family))
}
