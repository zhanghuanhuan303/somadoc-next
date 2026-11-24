//! The browser build: parse, enrich images, lay out, render fragments.
//!
//! This is the whole engine linked into wasm. There is no server round-trip for
//! layout; `/api/verify` only checks that the native build agrees.

use soma_render_html::PageFragment;
use soma_studio_core::{
    build as build_engine, enrich_images, image_sources, parse_document, BuildOptions, ImageBytes,
    ProjectPayload, SourceFormat,
};

use crate::api;

pub struct AppBuild {
    pub build: soma_studio_core::Build,
    pub fragments: Vec<PageFragment>,
    /// Canonical DocIR with node spans. Kept beside the layout so source
    /// mapping does not re-serialize on every cursor move.
    pub docir: serde_json::Value,
    pub elapsed_ms: u64,
}

pub enum SourceInput<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
}

/// Build in the browser. Images are fetched from Axum and inlined into the
/// page fragments, so a proof never points at a remote resource.
pub async fn build(
    project: &ProjectPayload,
    input: SourceInput<'_>,
    template: &str,
    engine: Option<&str>,
    metrics: Option<&[u8]>,
) -> Result<AppBuild, String> {
    let started = js_sys::Date::now();
    let bytes: &[u8] = match input {
        SourceInput::Text(text) => {
            if project.format == SourceFormat::Docx {
                return Err(String::from("DOCX 源文是二进制，不能从文本编辑"));
            }
            text.as_bytes()
        }
        SourceInput::Bytes(bytes) => bytes,
    };

    let mut parsed = parse_document(project.format, bytes)?;
    let mut images = ImageBytes::from_media(&parsed.media);
    for src in image_sources(&parsed) {
        if images.get(&src).is_some() {
            continue;
        }
        if let Ok(bytes) = api::asset(&src).await {
            images.insert(src, bytes);
        }
    }
    enrich_images(&mut parsed, &images);

    let build = build_engine(
        parsed,
        BuildOptions {
            template,
            engine,
            metrics,
            hyphenation: None,
        },
    )?;
    let fragments = build.fragments(&images);
    let docir =
        serde_json::from_str(&build.document.canonical_json()).unwrap_or(serde_json::Value::Null);
    let elapsed_ms = (js_sys::Date::now() - started).max(0.0) as u64;
    Ok(AppBuild {
        build,
        fragments,
        docir,
        elapsed_ms,
    })
}
