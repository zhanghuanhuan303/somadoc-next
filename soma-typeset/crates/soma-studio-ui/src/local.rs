//! Browser-local mode: no Axum round-trip for the engine.
//!
//! The wasm build already parses, lays out and renders the SVG proof. This
//! module supplies the pieces the server used to add: the default project,
//! the metrics table, hyphenation, local verification, TikZ and PDF.
//!
//! Fonts are the only large payload. They are static files under `/fonts/`
//! (copied next to `dist/` at deploy time) and are fetched once when the first
//! PDF export asks for them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use gloo_net::http::Request;
use leptos::prelude::*;
use soma_render_pdf::{render, PdfFont, PdfFonts, PdfMetadata};
use soma_studio_core::{
    diagram_tikz, parse_document, Build, BuildOptions, FileEntry, FontInfo, MapImages,
    NativeSummary, ProjectPayload, SourceFormat,
};

use crate::state::Studio;

/// The document the standalone build opens. It is small enough to embed and
/// exercises the full template (headings, tables, figures, footnotes).
pub const SOURCE: &str = include_str!("../../../examples/book/full.tex");
pub const TEMPLATE: &str = include_str!("../../../examples/templates/plex.yaml");
pub const ENGINE: &str = include_str!("../../../examples/templates/proposal.engine");
pub const METRICS: &[u8] = include_bytes!("../../../examples/templates/out/plex.smt");
pub const HYPHENATION: &str = include_str!("../../../examples/book/hyphen-en.pat");

const PDF_PLEX_REGULAR: &str = "/fonts/v1/IBMPlexSansSC_Regular.ttf";
const PDF_PLEX_BOLD: &str = "/fonts/v1/IBMPlexSansSC_Bold.ttf";
const PDF_MONO_REGULAR: &str = "/fonts/v1/NotoSansMono-Regular.ttf";
const PDF_MONO_BOLD: &str = "/fonts/v1/NotoSansMono-Bold.ttf";
const PDF_MATH: &str = "/fonts/v1/NewCMMath-Regular.otf";

/// The proof only needs the glyphs this document (and the Studio chrome) uses.
/// A full CJK face is 19 MB; the subset is under 0.5 MB and keeps the first
/// paint local. PDF export still fetches the complete faces below.
const DISPLAY_PLEX_REGULAR: &str = "/fonts/v1/subset/IBMPlexSansSC_Regular.woff2";
const DISPLAY_PLEX_BOLD: &str = "/fonts/v1/subset/IBMPlexSansSC_Bold.woff2";
const DISPLAY_MONO_REGULAR: &str = "/fonts/v1/subset/NotoSansMono-Regular.woff2";
const DISPLAY_MONO_BOLD: &str = "/fonts/v1/subset/NotoSansMono-Bold.woff2";
const DISPLAY_MATH: &str = "/fonts/v1/subset/NewCMMath-Regular.woff2";

/// Unique faces fetched for PDF embedding. `serif`/`sans` and `fallback`
/// share the Plex face; the PDF writer embeds each font once per role anyway.
const PDF_FONTS: &[(&str, &str)] = &[
    ("plex", PDF_PLEX_REGULAR),
    ("plex-bold", PDF_PLEX_BOLD),
    ("mono", PDF_MONO_REGULAR),
    ("mono-bold", PDF_MONO_BOLD),
    ("math", PDF_MATH),
];

/// PDF faces subsetted to the embedded document plus the template strings.
/// They are used only when the current source is covered by the subset; an
/// edited document outside that set falls back to the full faces above.
const PDF_SUBSET_FONTS: &[(&str, &str)] = &[
    ("plex", "/fonts/v1/pdf-subset/IBMPlexSansSC_Regular.ttf"),
    ("plex-bold", "/fonts/v1/pdf-subset/IBMPlexSansSC_Bold.ttf"),
    ("mono", "/fonts/v1/pdf-subset/NotoSansMono-Regular.ttf"),
    ("mono-bold", "/fonts/v1/pdf-subset/NotoSansMono-Bold.ttf"),
    ("math", "/fonts/v1/pdf-subset/NewCMMath-Regular.otf"),
];

/// True on the public somadoc hosts unless the page explicitly asks for the
/// server (`?server=1`). Loopback and LAN hosts stay on the server path: their
/// gates and the developer workflow expect the file tree and native check.
/// Any other host can opt in with `?local=1`.
pub fn is_local() -> bool {
    let Some(window) = web_sys::window() else {
        return true;
    };
    let Ok(search) = window.location().search() else {
        return true;
    };
    if search.contains("server=1") {
        return false;
    }
    if search.contains("local=1") {
        return true;
    }
    let host = window.location().hostname().unwrap_or_default();
    host == "somadoc.art" || host.ends_with(".somadoc.art")
}

pub fn project() -> ProjectPayload {
    ProjectPayload {
        schema: String::from("soma.studio.project/1"),
        version: 1,
        engine_version: String::from(env!("CARGO_PKG_VERSION")),
        root: String::from("local"),
        input: String::from("full.tex"),
        input_relative: String::from("full.tex"),
        format: SourceFormat::Latex,
        source_text: Some(SOURCE.to_string()),
        template_path: Some(String::from("examples/templates/plex.yaml")),
        template: TEMPLATE.to_string(),
        engine_path: Some(String::from("examples/templates/proposal.engine")),
        engine: Some(ENGINE.to_string()),
        fonts: fonts(),
        capabilities: Vec::new(),
        native: NativeSummary::default(),
        native_error: None,
        pdf: true,
        custom_metrics: true,
        custom_hyphenation: true,
    }
}

fn fonts() -> Vec<FontInfo> {
    vec![
        font(
            "fallback",
            "IBM Plex Sans SC",
            DISPLAY_PLEX_REGULAR,
            "normal",
        ),
        font("math", "NewComputerModernMath", DISPLAY_MATH, "normal"),
        font("mono", "Noto Sans Mono", DISPLAY_MONO_REGULAR, "normal"),
        font("mono-bold", "Noto Sans Mono", DISPLAY_MONO_BOLD, "bold"),
        font("sans", "IBM Plex Sans SC", DISPLAY_PLEX_REGULAR, "normal"),
        font("sans-bold", "IBM Plex Sans SC", DISPLAY_PLEX_BOLD, "bold"),
        font("serif", "IBM Plex Sans SC", DISPLAY_PLEX_REGULAR, "normal"),
        font("serif-bold", "IBM Plex Sans SC", DISPLAY_PLEX_BOLD, "bold"),
    ]
}

fn font(role: &str, family: &str, url: &str, weight: &str) -> FontInfo {
    FontInfo {
        role: String::from(role),
        family: String::from(family),
        url: String::from(url),
        weight: String::from(weight),
        style: String::from("normal"),
    }
}

pub fn tree() -> Vec<FileEntry> {
    vec![FileEntry {
        path: String::from("full.tex"),
        name: String::from("full.tex"),
        kind: String::from("file"),
        size: SOURCE.len() as u64,
    }]
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|error| format!("{url}: {error}"))?;
    if !response.ok() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    response
        .binary()
        .await
        .map_err(|error| format!("{url}: {error}"))
}

/// Fetch an image the DocIR points at. Same-origin static files only; remote
/// images and `data:` URIs are already carried by `ImageBytes` from the source.
pub async fn asset(src: &str) -> Result<Vec<u8>, String> {
    if src.contains("://") || src.starts_with("data:") {
        return Err(format!("local mode cannot fetch {src}"));
    }
    let url = format!("/{}", src.trim_start_matches('/'));
    fetch(&url).await
}

async fn font_cache(studio: Studio) -> Result<Arc<BTreeMap<String, Vec<u8>>>, String> {
    if let Some(cache) = studio.pdf_fonts.get_untracked() {
        return Ok(cache);
    }
    let cache = Arc::new(fetch_fonts(PDF_FONTS).await?);
    studio.pdf_fonts.set(Some(cache.clone()));
    Ok(cache)
}

async fn fetch_fonts(urls: &[(&str, &str)]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut map = BTreeMap::new();
    for (name, url) in urls {
        map.insert((*name).to_string(), fetch(url).await?);
    }
    Ok(map)
}

fn subset_covers(studio: Studio) -> bool {
    let active = studio.active.get_untracked();
    let Some(tab) = studio.tabs.with_untracked(|tabs| tabs.get(active).cloned()) else {
        return true;
    };
    if tab.format == SourceFormat::Docx {
        return false;
    }
    let coverage: BTreeSet<char> = SOURCE
        .chars()
        .chain(TEMPLATE.chars())
        .chain(ENGINE.chars())
        .collect();
    studio
        .source
        .get_untracked()
        .chars()
        .all(|ch| coverage.contains(&ch))
}

/// Render the current build to PDF entirely in wasm. The caller downloads the
/// bytes; nothing crosses the network except the cached font files.
pub async fn pdf(studio: Studio) -> Result<Vec<u8>, String> {
    let cache = if subset_covers(studio) {
        Arc::new(fetch_fonts(PDF_SUBSET_FONTS).await?)
    } else {
        font_cache(studio).await?
    };
    let bytes = studio.build.with_untracked(|app| {
        let Some(app) = app.as_ref() else {
            return Err(String::from("还没有可导出的构建"));
        };
        let build: &Build = &app.build;
        let plex = PdfFont::from_bytes(cache["plex"].clone(), 0)
            .map_err(|error| format!("IBM Plex Sans SC: {error}"))?;
        let plex_bold = PdfFont::from_bytes(cache["plex-bold"].clone(), 0)
            .map_err(|error| format!("IBM Plex Sans SC Bold: {error}"))?;
        let mono = PdfFont::from_bytes(cache["mono"].clone(), 0)
            .map_err(|error| format!("Noto Sans Mono: {error}"))?;
        let mono_bold = PdfFont::from_bytes(cache["mono-bold"].clone(), 0)
            .map_err(|error| format!("Noto Sans Mono Bold: {error}"))?;
        let math = PdfFont::from_bytes(cache["math"].clone(), 0)
            .map_err(|error| format!("NewComputerModernMath: {error}"))?;
        let fonts = PdfFonts {
            regular: &plex,
            bold: Some(&plex_bold),
            sans: Some(&plex),
            sans_bold: Some(&plex_bold),
            mono: Some(&mono),
            mono_bold: Some(&mono_bold),
            math: Some(&math),
            fallback: Some(&plex),
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
        let images = MapImages::new(&app.images);
        render(
            &build.pages,
            &build.styled,
            &build.pages.text,
            &fonts,
            &metadata,
            &images,
        )
        .map_err(|error| format!("pdf: {error}"))
    })?;
    Ok(bytes)
}

/// Compile the first diagram in the current tab to TikZ, using the same
/// template, metrics and engine parameters as the proof.
pub fn tikz(studio: Studio) -> Result<String, String> {
    let active = studio.active.get_untracked();
    let tab = studio
        .tabs
        .with_untracked(|tabs| tabs.get(active).cloned())
        .ok_or_else(|| String::from("没有打开的文档"))?;
    let bytes = if tab.format == SourceFormat::Docx {
        studio
            .docx_bytes
            .get_untracked()
            .ok_or_else(|| String::from("无法读取 DOCX 源文件"))?
    } else {
        studio.source.get_untracked().into_bytes()
    };
    let parsed = parse_document(tab.format, &bytes)?;
    let template = studio.template.get_untracked();
    let engine = studio.engine_text.get_untracked();
    let metrics = studio.metrics_bytes.get_untracked();
    let hyphenation = studio.hyphenation.get_untracked();
    diagram_tikz(
        &parsed,
        BuildOptions {
            template: &template,
            engine: engine.as_deref(),
            metrics: metrics.as_deref(),
            hyphenation: hyphenation.as_deref(),
        },
    )
}
