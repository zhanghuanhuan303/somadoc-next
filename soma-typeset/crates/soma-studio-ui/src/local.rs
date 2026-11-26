//! Browser-local mode: no Axum round-trip for the engine.
//!
//! The wasm build already parses, lays out and renders the SVG proof. This
//! module supplies the pieces the server used to add: the default project,
//! the metrics table, hyphenation, local verification, TikZ and PDF.
//!
//! Fonts are the only large payload. They are static files under `/fonts/`
//! (copied next to `dist/` at deploy time) and are fetched once when the first
//! PDF export asks for them.

use std::collections::BTreeMap;
use std::sync::Arc;

use gloo_net::http::Request;
use leptos::prelude::*;
use soma_render_pdf::{render, PdfFont, PdfFonts, PdfMetadata};
use soma_studio_core::{
    diagram_tikz, parse_document, Build, BuildOptions, FileEntry, FontInfo, MapImages,
    NativeSummary, ProjectPayload, SourceFormat,
};

use crate::state::Studio;

/// The single document the standalone build opens. It is the user's entry
/// point: replace the prose, keep editing, and the proof follows.
pub const SOURCE: &str = include_str!("../../../examples/book/welcome.md");
pub const TEMPLATE: &str = include_str!("../../../examples/templates/plex.yaml");
pub const ENGINE: &str = include_str!("../../../examples/templates/proposal.engine");
pub const METRICS: &[u8] = include_bytes!("../../../examples/templates/plex.smt");
pub const HYPHENATION: &str = include_str!("../../../examples/book/hyphen-en.pat");

/// GB2312 levels 1+2 subsets (6 763 characters), shared by the proof and PDF.
/// The same bytes are served to `@font-face`, so exporting a PDF does not
/// download a second, larger font. Characters outside GB2312 render as empty
/// boxes in the browser; the native CLI still has the full faces.
const PLEX_REGULAR: &str = "/fonts/v2/IBMPlexSansSC_Regular.ttf";
const PLEX_BOLD: &str = "/fonts/v2/IBMPlexSansSC_Bold.ttf";
const MONO_REGULAR: &str = "/fonts/v2/NotoSansMono-Regular.ttf";
const MONO_BOLD: &str = "/fonts/v2/NotoSansMono-Bold.ttf";
const MATH: &str = "/fonts/v2/NewCMMath-Regular.otf";

const FONTS: &[(&str, &str)] = &[
    ("plex", PLEX_REGULAR),
    ("plex-bold", PLEX_BOLD),
    ("mono", MONO_REGULAR),
    ("mono-bold", MONO_BOLD),
    ("math", MATH),
];

/// True unless the page is on loopback, where the native host is the developer
/// workflow. The public bundle is served from hosts we do not know in advance
/// (somadoc.art, a CDN preview, a custom domain), so local must be the default:
/// a host allowlist would make those deployments silently call `/api`.
///
/// `?server=1` forces the server anywhere; `?local=1` forces the browser-local
/// engine, including on loopback for the static gate.
pub fn is_local() -> bool {
    let Some(window) = web_sys::window() else {
        return true;
    };
    let search = window.location().search().unwrap_or_default();
    let host = window.location().hostname().unwrap_or_default();
    resolve_local(&search, &host)
}

fn resolve_local(search: &str, host: &str) -> bool {
    let flag = |name: &str| {
        search
            .trim_start_matches('?')
            .split('&')
            .any(|pair| pair == format!("{name}=1"))
    };
    if flag("server") {
        return false;
    }
    if flag("local") {
        return true;
    }
    !is_loopback(host)
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host.ends_with(".localhost")
}

pub fn project() -> ProjectPayload {
    ProjectPayload {
        schema: String::from("soma.studio.project/1"),
        version: 1,
        engine_version: String::from(env!("CARGO_PKG_VERSION")),
        root: String::from("local"),
        input: String::from("welcome.md"),
        input_relative: String::from("welcome.md"),
        format: SourceFormat::Markdown,
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
        font("fallback", "IBM Plex Sans SC", PLEX_REGULAR, "normal"),
        font("math", "NewComputerModernMath", MATH, "normal"),
        font("mono", "Noto Sans Mono", MONO_REGULAR, "normal"),
        font("mono-bold", "Noto Sans Mono", MONO_BOLD, "bold"),
        font("sans", "IBM Plex Sans SC", PLEX_REGULAR, "normal"),
        font("sans-bold", "IBM Plex Sans SC", PLEX_BOLD, "bold"),
        font("serif", "IBM Plex Sans SC", PLEX_REGULAR, "normal"),
        font("serif-bold", "IBM Plex Sans SC", PLEX_BOLD, "bold"),
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
        path: String::from("welcome.md"),
        name: String::from("welcome.md"),
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
    let cache = Arc::new(fetch_fonts(FONTS).await?);
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

/// Render the current build to PDF entirely in wasm. The GB2312 faces are the
/// same files the proof uses, so the browser usually serves them from cache and
/// there is no coverage check or fallback download.
pub async fn pdf(studio: Studio) -> Result<Vec<u8>, String> {
    let cache = font_cache(studio).await?;
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

#[cfg(test)]
mod tests {
    use super::resolve_local;

    #[test]
    fn public_hosts_default_to_local() {
        assert!(resolve_local("", "somadoc.art"));
        assert!(resolve_local("", "www.somadoc.art"));
        assert!(resolve_local("", "preview.example.net"));
        assert!(resolve_local("", "192.0.2.10"));
    }

    #[test]
    fn loopback_keeps_the_development_server() {
        assert!(!resolve_local("", "localhost"));
        assert!(!resolve_local("", "127.0.0.1"));
        assert!(!resolve_local("", "[::1]"));
        assert!(resolve_local("?local=1", "127.0.0.1"));
    }

    #[test]
    fn explicit_flags_win() {
        assert!(!resolve_local("?server=1", "somadoc.art"));
        assert!(!resolve_local("?server=1", "127.0.0.1"));
        assert!(resolve_local("?local=1", "preview.example.net"));
        assert!(resolve_local("?theme=dark&local=1", "preview.example.net"));
    }
}
