//! `soma-studio`: Axum boundary for the all-Rust wasm workbench.
//!
//! The browser owns parsing, templating, line breaking, pagination and HTML.
//! This process owns the filesystem, fonts, native verification and PDF. Both
//! sides run `soma-studio-core`; their canonical hashes must match.

mod pdf;

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use axum::extract::{Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::{ServeDir, ServeFile};

use soma_studio_core::{
    build, diagram_tikz, enrich_images, image_sources, parse_document, Build, BuildOptions,
    BuildRequest, CapabilityInfo, FontInfo, ImageBytes, NativeSummary, ProjectPayload, SaveRequest,
    SourceFormat, TreeResponse,
};

const DEFAULT_TEMPLATE: &str = include_str!("../../../examples/templates/plex.yaml");
const EMBEDDED_MATH: &[u8] = include_bytes!("../../soma-cli/assets/NewCMMath-Regular.otf");

#[derive(Clone)]
struct Options {
    input: PathBuf,
    root: PathBuf,
    template: Option<PathBuf>,
    engine: Option<PathBuf>,
    metrics: Option<PathBuf>,
    hyphenation: Option<PathBuf>,
    host: String,
    port: u16,
    dist: PathBuf,
    open: bool,
    watch: bool,
    auto_fonts: bool,
}

#[derive(Clone, Default)]
struct FontSpecs {
    serif: Option<String>,
    serif_bold: Option<String>,
    sans: Option<String>,
    sans_bold: Option<String>,
    mono: Option<String>,
    mono_bold: Option<String>,
    math: Option<String>,
    fallback: Option<String>,
}

#[derive(Clone)]
struct FontEntry {
    info: FontInfo,
    bytes: Vec<u8>,
    index: u32,
}

struct Bundle {
    project: ProjectPayload,
    source: Vec<u8>,
    template: String,
    engine: Option<String>,
    metrics: Option<Vec<u8>>,
    hyphenation: Option<String>,
    version: u64,
}

struct AppState {
    options: Options,
    fonts: BTreeMap<String, FontEntry>,
    bundle: RwLock<Bundle>,
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    let (options, specs) = match parse_args(std::env::args().skip(1).collect()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("soma-studio: {error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let fonts = match load_fonts(&specs, options.auto_fonts) {
        Ok(fonts) => fonts,
        Err(error) => {
            eprintln!("soma-studio: {error}");
            std::process::exit(2);
        }
    };
    let metrics = match read_metrics(&options) {
        Ok(metrics) => metrics,
        Err(error) => {
            eprintln!("soma-studio: {error}");
            std::process::exit(2);
        }
    };
    let bundle = match load_bundle(&options, &fonts, metrics, 1, false) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("soma-studio: {error}");
            std::process::exit(2);
        }
    };
    let (tx, _) = broadcast::channel(32);
    let state = Arc::new(AppState {
        options,
        fonts,
        bundle: RwLock::new(bundle),
        tx,
    });

    if !state.options.dist.join("index.html").exists() {
        eprintln!(
            "soma-studio: warning: {} has no index.html; run `trunk build` in crates/soma-studio-ui",
            state.options.dist.display()
        );
    }

    let index = state.options.dist.join("index.html");
    let static_files = ServeDir::new(&state.options.dist).not_found_service(ServeFile::new(index));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/project", get(project))
        .route("/api/tree", get(tree))
        .route("/api/file", get(read_file).post(write_file))
        .route("/api/source", get(source))
        .route("/api/asset", get(asset))
        .route("/api/metrics", get(metrics_file))
        .route("/api/hyphenation", get(hyphenation_file))
        .route("/api/font/{role}", get(font))
        .route("/api/verify", post(verify))
        .route("/api/pdf", post(export_pdf))
        .route("/api/tikz", post(export_tikz))
        .route("/api/rebuild", post(rebuild))
        .route("/api/events", get(events))
        .fallback_service(static_files)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .with_state(state.clone());

    if state.options.watch {
        tokio::spawn(watch(state.clone()));
    }

    let address = authority(&state.options.host, state.options.port);
    let listener = match TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("soma-studio: cannot bind {address}: {error}");
            std::process::exit(1);
        }
    };
    let display = if is_wildcard(&state.options.host) {
        "127.0.0.1"
    } else {
        state.options.host.as_str()
    };
    let url = format!("http://{}", authority(display, state.options.port));
    println!("soma-studio: {url}");
    if is_wildcard(&state.options.host) {
        println!("soma-studio: listening on every interface ({address})");
    }
    println!("soma-studio: input {}", state.options.input.display());
    if let Some(path) = &state.options.hyphenation {
        println!("soma-studio: hyphenation {}", path.display());
    }
    if state.options.open {
        open_browser(&url);
    }
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("soma-studio: {error}");
    }
}

/* --------------------------------------------------------------------- args */

const USAGE: &str = "\
soma-studio — all-Rust wasm workbench with an Axum I/O boundary

usage:
  soma-studio --input <file> [--template <yaml>] [--engine <yaml>]
              [--metrics <smt>]
              [--hyphenation <patterns>]
              [--font <file[#index]>] [--font-bold <file[#index]>]
              [--font-sans <file[#index]>] [--font-sans-bold <file[#index]>]
              [--font-mono <file[#index]>] [--font-mono-bold <file[#index]>]
              [--font-math <file[#index]>] [--font-fallback <file[#index]>]
              [--host 127.0.0.1] [--port 7070] [--dist <dir>]
              [--open] [--no-watch] [--no-auto-font]

The UI is built with `trunk build` in crates/soma-studio-ui; Axum serves dist/.
";

fn parse_args(args: Vec<String>) -> Result<(Options, FontSpecs), String> {
    let mut input = None;
    let mut template = None;
    let mut engine = None;
    let mut metrics = None;
    let mut hyphenation = None;
    let mut host = String::from("127.0.0.1");
    let mut port = 7070u16;
    let mut dist = None;
    let mut open = false;
    let mut watch = true;
    let mut auto_fonts = true;
    let mut specs = FontSpecs::default();

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg {
            "--input" | "-i" => input = Some(PathBuf::from(value()?)),
            "--template" | "-t" => template = Some(PathBuf::from(value()?)),
            "--engine" => engine = Some(PathBuf::from(value()?)),
            "--metrics" => metrics = Some(PathBuf::from(value()?)),
            "--hyphenation" => hyphenation = Some(PathBuf::from(value()?)),
            "--font" => specs.serif = Some(value()?),
            "--font-bold" => specs.serif_bold = Some(value()?),
            "--font-sans" => specs.sans = Some(value()?),
            "--font-sans-bold" => specs.sans_bold = Some(value()?),
            "--font-mono" => specs.mono = Some(value()?),
            "--font-mono-bold" => specs.mono_bold = Some(value()?),
            "--font-math" => specs.math = Some(value()?),
            "--font-fallback" => specs.fallback = Some(value()?),
            "--host" => {
                let parsed = value()?;
                if parsed.trim().is_empty() {
                    return Err(String::from("--host must not be empty"));
                }
                host = parsed;
            }
            "--port" => {
                port = value()?
                    .parse()
                    .map_err(|_| String::from("--port must be a number"))?
            }
            "--dist" => dist = Some(PathBuf::from(value()?)),
            "--open" => open = true,
            "--no-watch" => watch = false,
            "--no-auto-font" => auto_fonts = false,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
        index += 1;
    }

    let input = input.ok_or_else(|| String::from("--input is required"))?;
    let input = input
        .canonicalize()
        .map_err(|error| format!("{}: {error}", input.display()))?;
    let root = input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let dist = dist.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../soma-studio-ui/dist")
    });
    let dist = dist.canonicalize().unwrap_or(dist);
    let hyphenation = match hyphenation {
        Some(path) => Some(
            path.canonicalize()
                .map_err(|error| format!("{}: {error}", path.display()))?,
        ),
        None => {
            let probe = root.join("hyphen-en.pat");
            probe
                .is_file()
                .then(|| probe.canonicalize().unwrap_or(probe))
        }
    };
    let options = Options {
        input,
        root,
        template,
        engine,
        metrics,
        hyphenation,
        host,
        port,
        dist,
        open,
        watch,
        auto_fonts,
    };
    Ok((options, specs))
}

fn open_browser(url: &str) {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "cmd"
    } else {
        "xdg-open"
    };
    let mut process = std::process::Command::new(command);
    if cfg!(windows) {
        process.args(["/C", "start", "", url]);
    } else {
        process.arg(url);
    }
    let _ = process.spawn();
}

/* -------------------------------------------------------------------- fonts */

fn load_fonts(specs: &FontSpecs, auto_fonts: bool) -> Result<BTreeMap<String, FontEntry>, String> {
    let mut fonts = BTreeMap::new();
    insert_font(&mut fonts, "serif", specs.serif.as_ref(), "normal")?;
    insert_font(&mut fonts, "serif-bold", specs.serif_bold.as_ref(), "bold")?;
    insert_font(&mut fonts, "sans", specs.sans.as_ref(), "normal")?;
    insert_font(&mut fonts, "sans-bold", specs.sans_bold.as_ref(), "bold")?;
    insert_font(&mut fonts, "mono", specs.mono.as_ref(), "normal")?;
    insert_font(&mut fonts, "mono-bold", specs.mono_bold.as_ref(), "bold")?;
    insert_font(&mut fonts, "fallback", specs.fallback.as_ref(), "normal")?;

    // No explicit body face: use a system CJK face so a PDF is never blank
    // just because the caller forgot --font. The browser gets the family name
    // (no URL), so it draws with the installed face; the PDF backend still
    // subsets the file it embeds.
    if auto_fonts && !fonts.contains_key("serif") {
        if let Some(entry) = auto_entry(
            "serif",
            "Noto Serif CJK SC",
            "Noto Serif CJK SC",
            "normal",
            '中',
        )?
        .or(auto_entry(
            "serif",
            "Noto Sans CJK SC",
            "Noto Sans CJK SC",
            "normal",
            '中',
        )?) {
            fonts.insert(String::from("serif"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("serif-bold") {
        if let Some(entry) = auto_entry(
            "serif-bold",
            "Noto Serif CJK SC:style=Bold",
            "Noto Serif CJK SC",
            "bold",
            '中',
        )? {
            fonts.insert(String::from("serif-bold"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("sans") {
        if let Some(entry) = auto_entry(
            "sans",
            "Noto Sans CJK SC",
            "Noto Sans CJK SC",
            "normal",
            '中',
        )? {
            fonts.insert(String::from("sans"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("sans-bold") {
        if let Some(entry) = auto_entry(
            "sans-bold",
            "Noto Sans CJK SC:style=Bold",
            "Noto Sans CJK SC",
            "bold",
            '中',
        )? {
            fonts.insert(String::from("sans-bold"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("mono") {
        if let Some(entry) = auto_entry("mono", "Noto Sans Mono", "Noto Sans Mono", "normal", 'A')?
        {
            fonts.insert(String::from("mono"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("mono-bold") {
        if let Some(entry) = auto_entry(
            "mono-bold",
            "Noto Sans Mono:style=Bold",
            "Noto Sans Mono",
            "bold",
            'A',
        )? {
            fonts.insert(String::from("mono-bold"), entry);
        }
    }
    if auto_fonts && !fonts.contains_key("fallback") {
        let body_covers_cjk = fonts
            .get("serif")
            .map(|entry| font_has_glyph(entry, '中'))
            .unwrap_or(false);
        if !body_covers_cjk {
            if let Some(entry) = auto_entry(
                "fallback",
                "Noto Sans CJK SC",
                "Noto Sans CJK SC",
                "normal",
                '中',
            )? {
                fonts.insert(String::from("fallback"), entry);
            }
        }
    }

    if let Some(spec) = specs.math.as_ref() {
        insert_font(&mut fonts, "math", Some(spec), "normal")?;
    } else {
        fonts.insert(
            String::from("math"),
            FontEntry {
                info: FontInfo {
                    role: String::from("math"),
                    family: String::from("New Computer Modern Math"),
                    url: String::from("/api/font/math"),
                    weight: String::from("normal"),
                    style: String::from("normal"),
                },
                bytes: EMBEDDED_MATH.to_vec(),
                index: 0,
            },
        );
    }
    Ok(fonts)
}

fn insert_font(
    fonts: &mut BTreeMap<String, FontEntry>,
    role: &str,
    spec: Option<&String>,
    weight: &str,
) -> Result<(), String> {
    let Some(spec) = spec else {
        return Ok(());
    };
    fonts.insert(role.to_string(), load_font_entry(role, spec, weight)?);
    Ok(())
}

/// Resolve a family with fontconfig. The returned spec carries `#index`, which
/// `load_font_entry` already understands for TTC faces.
fn auto_font(query: &str) -> Option<String> {
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}#%{index}", query])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let spec = String::from_utf8(output.stdout).ok()?;
    let spec = spec.trim();
    let (path, _) = split_font_spec(spec).ok()?;
    std::path::Path::new(path)
        .is_file()
        .then(|| spec.to_string())
}

fn auto_entry(
    role: &str,
    query: &str,
    display: &str,
    weight: &str,
    required: char,
) -> Result<Option<FontEntry>, String> {
    let Some(spec) = auto_font(query) else {
        return Ok(None);
    };
    let Ok(mut entry) = load_font_entry(role, &spec, weight) else {
        return Ok(None);
    };
    if !font_has_glyph(&entry, required) {
        return Ok(None);
    }
    // A system family is referenced by name; do not serve a 20 MB TTC to the
    // browser. The PDF still embeds a glyph subset of the same file.
    entry.info.family = display.to_string();
    entry.info.url.clear();
    Ok(Some(entry))
}

fn font_has_glyph(entry: &FontEntry, character: char) -> bool {
    soma_render_pdf::PdfFont::from_bytes(entry.bytes.clone(), entry.index)
        .map(|font| font.has_glyph(character as u32))
        .unwrap_or(false)
}

fn load_font_entry(role: &str, spec: &str, weight: &str) -> Result<FontEntry, String> {
    let (path, index) = split_font_spec(spec)?;
    let bytes = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    let family = soma_studio_core::font_family_name(&bytes)
        .filter(|family| !family.trim().is_empty())
        .unwrap_or_else(|| {
            Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(role)
                .to_string()
        });
    Ok(FontEntry {
        info: FontInfo {
            role: role.to_string(),
            family,
            url: format!("/api/font/{role}"),
            weight: weight.to_string(),
            style: String::from("normal"),
        },
        bytes,
        index,
    })
}

fn split_font_spec(spec: &str) -> Result<(&str, u32), String> {
    match spec.rsplit_once('#') {
        Some((path, index)) => Ok((
            path,
            index
                .parse::<u32>()
                .map_err(|_| format!("{spec}: face index must be a number"))?,
        )),
        None => Ok((spec, 0)),
    }
}

fn read_metrics(options: &Options) -> Result<Option<Vec<u8>>, String> {
    match &options.metrics {
        Some(path) => std::fs::read(path)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        None => Ok(None),
    }
}

fn read_hyphenation(options: &Options) -> Result<Option<String>, String> {
    match &options.hyphenation {
        Some(path) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        None => Ok(None),
    }
}

/* ------------------------------------------------------------------- bundle */

fn load_bundle(
    options: &Options,
    fonts: &BTreeMap<String, FontEntry>,
    metrics: Option<Vec<u8>>,
    version: u64,
    strict: bool,
) -> Result<Bundle, String> {
    let source = std::fs::read(&options.input)
        .map_err(|error| format!("{}: {error}", options.input.display()))?;
    let template = match &options.template {
        Some(path) => {
            std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?
        }
        None => DEFAULT_TEMPLATE.to_string(),
    };
    let engine = match &options.engine {
        Some(path) => Some(
            std::fs::read_to_string(path)
                .map_err(|error| format!("{}: {error}", path.display()))?,
        ),
        None => None,
    };
    let format = SourceFormat::from_path(
        options
            .input
            .to_str()
            .ok_or_else(|| String::from("input path is not UTF-8"))?,
    )
    .ok_or_else(|| {
        format!(
            "{}: unsupported input (expected md, html, tex, or docx)",
            options.input.display()
        )
    })?;
    let hyphenation = read_hyphenation(options)?;

    let started = Instant::now();
    let (native, native_error) = match native_build(
        format,
        &source,
        &template,
        engine.as_deref(),
        metrics.as_deref(),
        hyphenation.as_deref(),
        &options.root,
    ) {
        Ok(build) => (build.summary(started.elapsed().as_millis() as u64), None),
        Err(error) => (NativeSummary::default(), Some(error)),
    };

    if strict {
        if let Some(error) = native_error.as_ref() {
            return Err(format!("build: {error}"));
        }
    }

    let source_text = if format == SourceFormat::Docx {
        None
    } else {
        String::from_utf8(source.clone()).ok()
    };
    let input_relative = options
        .input
        .strip_prefix(&options.root)
        .unwrap_or(&options.input)
        .to_string_lossy()
        .replace('\\', "/");
    let capabilities = soma_md::CAPABILITIES
        .iter()
        .map(|capability| CapabilityInfo {
            syntax: capability.syntax.to_string(),
            status: capability.status.as_str().to_string(),
            note: capability.note.to_string(),
        })
        .collect();
    let fonts_info = fonts.values().map(|entry| entry.info.clone()).collect();
    let project = ProjectPayload {
        schema: String::from("soma.studio/1"),
        version,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        root: options.root.to_string_lossy().to_string(),
        input: options.input.to_string_lossy().to_string(),
        input_relative,
        format,
        source_text,
        template_path: options
            .template
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        template,
        engine_path: options
            .engine
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        engine,
        fonts: fonts_info,
        capabilities,
        native,
        native_error,
        pdf: fonts.contains_key("serif"),
        custom_metrics: metrics.is_some(),
        custom_hyphenation: hyphenation.is_some(),
    };

    Ok(Bundle {
        project,
        source,
        template: match &options.template {
            Some(path) => std::fs::read_to_string(path).unwrap_or_default(),
            None => DEFAULT_TEMPLATE.to_string(),
        },
        engine: match &options.engine {
            Some(path) => std::fs::read_to_string(path).ok(),
            None => None,
        },
        metrics,
        hyphenation,
        version,
    })
}

fn native_build(
    format: SourceFormat,
    source: &[u8],
    template: &str,
    engine: Option<&str>,
    metrics: Option<&[u8]>,
    hyphenation: Option<&str>,
    root: &Path,
) -> Result<Build, String> {
    let mut parsed = parse_document(format, source)?;
    let mut images = ImageBytes::from_media(&parsed.media);
    for src in image_sources(&parsed) {
        if images.get(&src).is_some() {
            continue;
        }
        if let Some(path) = safe_join(root, &src) {
            if let Ok(bytes) = std::fs::read(path) {
                images.insert(src, bytes);
            }
        }
    }
    enrich_images(&mut parsed, &images);
    build(
        parsed,
        BuildOptions {
            template,
            engine,
            metrics,
            hyphenation,
        },
    )
}

/* ----------------------------------------------------------------- handlers */

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn project(State(state): State<Arc<AppState>>) -> Json<ProjectPayload> {
    Json(state.bundle.read().unwrap().project.clone())
}

async fn tree(State(state): State<Arc<AppState>>) -> Json<TreeResponse> {
    let mut entries = Vec::new();
    walk_tree(&state.options.root, &state.options.root, 0, &mut entries);
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Json(TreeResponse {
        root: state.options.root.to_string_lossy().to_string(),
        entries,
    })
}

fn walk_tree(
    root: &Path,
    directory: &Path,
    depth: usize,
    out: &mut Vec<soma_studio_core::FileEntry>,
) {
    if depth > 3 {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || matches!(name.as_str(), "target" | "dist" | "node_modules") {
            continue;
        }
        if path.is_dir() {
            walk_tree(root, &path, depth + 1, out);
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "md" | "markdown" | "tex" | "latex" | "html" | "htm" | "docx"
        ) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(soma_studio_core::FileEntry {
            path: relative,
            name,
            kind: String::from("file"),
            size: metadata.len(),
        });
    }
}

async fn read_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, (StatusCode, String)> {
    let path = query
        .get("path")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, String::from("path is required")))?;
    let path = safe_join(&state.options.root, path).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            String::from("path is outside the project"),
        )
    })?;
    std::fs::read_to_string(path).map_err(|error| (StatusCode::NOT_FOUND, error.to_string()))
}

async fn write_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<SaveRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let path = query
        .get("path")
        .ok_or_else(|| (StatusCode::BAD_REQUEST, String::from("path is required")))?;
    let path = safe_join(&state.options.root, path).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            String::from("path is outside the project"),
        )
    })?;
    std::fs::write(path, body.text)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

async fn source(State(state): State<Arc<AppState>>) -> Response {
    let bytes = state.bundle.read().unwrap().source.clone();
    bytes_response("application/octet-stream", bytes)
}

async fn asset(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let Some(path) = query.get("path") else {
        return (StatusCode::BAD_REQUEST, "path is required").into_response();
    };
    let Some(path) = safe_join(&state.options.root, path) else {
        return (StatusCode::FORBIDDEN, "path is outside the project").into_response();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return (StatusCode::NOT_FOUND, "asset not found").into_response();
    };
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    bytes_response(&mime, bytes)
}

async fn metrics_file(State(state): State<Arc<AppState>>) -> Response {
    let metrics = state.bundle.read().unwrap().metrics.clone();
    match metrics {
        Some(bytes) => bytes_response("application/octet-stream", bytes),
        None => (StatusCode::NOT_FOUND, "embedded metrics").into_response(),
    }
}

async fn hyphenation_file(State(state): State<Arc<AppState>>) -> Response {
    let hyphenation = state.bundle.read().unwrap().hyphenation.clone();
    match hyphenation {
        Some(text) => bytes_response("text/plain; charset=utf-8", text.into_bytes()),
        None => (StatusCode::NOT_FOUND, "no hyphenation patterns").into_response(),
    }
}

async fn font(State(state): State<Arc<AppState>>, AxumPath(role): AxumPath<String>) -> Response {
    let Some(entry) = state.fonts.get(&role) else {
        return (StatusCode::NOT_FOUND, "font role not configured").into_response();
    };
    let mime = if entry.bytes.starts_with(b"OTTO") {
        "font/otf"
    } else if entry.bytes.starts_with(&[0, 1, 0, 0]) || entry.bytes.starts_with(b"true") {
        "font/ttf"
    } else {
        "application/octet-stream"
    };
    bytes_response(mime, entry.bytes.clone())
}

async fn verify(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BuildRequest>,
) -> Result<Json<NativeSummary>, (StatusCode, String)> {
    let (format, source, template, engine, metrics, hyphenation) =
        request_input(&state, &request).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let started = Instant::now();
    let build = native_build(
        format,
        &source,
        &template,
        engine.as_deref(),
        metrics.as_deref(),
        hyphenation.as_deref(),
        &state.options.root,
    )
    .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(Json(build.summary(started.elapsed().as_millis() as u64)))
}

async fn export_pdf(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BuildRequest>,
) -> Result<Response, (StatusCode, String)> {
    let (format, source, template, engine, metrics, hyphenation) =
        request_input(&state, &request).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let build = native_build(
        format,
        &source,
        &template,
        engine.as_deref(),
        metrics.as_deref(),
        hyphenation.as_deref(),
        &state.options.root,
    )
    .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    let bytes = pdf::render_pdf(&build, &state.fonts, &state.options.root)
        .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    let mut response = bytes_response("application/pdf", bytes);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"soma-studio.pdf\""),
    );
    Ok(response)
}

async fn export_tikz(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BuildRequest>,
) -> Result<Response, (StatusCode, String)> {
    let (format, source, template, engine, metrics, hyphenation) =
        request_input(&state, &request).map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    let parsed = parse_document(format, &source)
        .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    let tikz = diagram_tikz(
        &parsed,
        BuildOptions {
            template: &template,
            engine: engine.as_deref(),
            metrics: metrics.as_deref(),
            hyphenation: hyphenation.as_deref(),
        },
    )
    .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    let mut response = bytes_response("text/plain; charset=utf-8", tikz.into_bytes());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"soma-diagram.tikz\""),
    );
    Ok(response)
}

type RequestInput = (
    SourceFormat,
    Vec<u8>,
    String,
    Option<String>,
    Option<Vec<u8>>,
    Option<String>,
);

fn request_input(state: &AppState, request: &BuildRequest) -> Result<RequestInput, String> {
    let bundle = state.bundle.read().unwrap();
    let format = request.format_or(bundle.project.format);
    let source = match &request.source {
        Some(text) => {
            if format == SourceFormat::Docx {
                return Err(String::from("DOCX is binary; omit source to use the file"));
            }
            text.as_bytes().to_vec()
        }
        None => std::fs::read(&state.options.input)
            .map_err(|error| format!("{}: {error}", state.options.input.display()))?,
    };
    let template = request
        .template
        .clone()
        .unwrap_or_else(|| bundle.template.clone());
    let engine = request.engine.clone().or_else(|| bundle.engine.clone());
    let metrics = bundle.metrics.clone();
    let hyphenation = bundle.hyphenation.clone();
    Ok((format, source, template, engine, metrics, hyphenation))
}

async fn rebuild(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ProjectPayload>, (StatusCode, String)> {
    let project = reload(&state).map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(Json(project))
}

fn reload(state: &Arc<AppState>) -> Result<ProjectPayload, String> {
    let version = state.bundle.read().unwrap().version + 1;
    let metrics = state.bundle.read().unwrap().metrics.clone();
    let bundle = load_bundle(&state.options, &state.fonts, metrics, version, true)?;
    let project = bundle.project.clone();
    *state.bundle.write().unwrap() = bundle;
    let _ = state
        .tx
        .send(json!({ "type": "built", "version": version }).to_string());
    Ok(project)
}

async fn events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|message| async move {
        message
            .ok()
            .map(|text| Ok::<_, Infallible>(Event::default().data(text)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn bytes_response(content_type: &str, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/* -------------------------------------------------------------------- watch */

fn snapshot(options: &Options) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut paths = vec![options.input.clone()];
    if let Some(path) = &options.template {
        paths.push(path.clone());
    }
    if let Some(path) = &options.engine {
        paths.push(path.clone());
    }
    if let Some(path) = &options.metrics {
        paths.push(path.clone());
    }
    if let Some(path) = &options.hyphenation {
        paths.push(path.clone());
    }
    paths
        .into_iter()
        .map(|path| {
            let modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok();
            (path, modified)
        })
        .collect()
}

async fn watch(state: Arc<AppState>) {
    let mut last = snapshot(&state.options);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let current = snapshot(&state.options);
        if current == last {
            continue;
        }
        last = current;
        if let Err(error) = reload(&state) {
            let _ = state
                .tx
                .send(json!({ "type": "error", "message": error }).to_string());
        }
    }
}

/* ------------------------------------------------------------------ security */

/// `host:port` with IPv6 literals bracketed.
fn authority(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

fn is_wildcard(host: &str) -> bool {
    matches!(host, "0.0.0.0" | "::" | "[::]")
}

/// Whether `Origin: <origin>` may drive `/api`.
///
/// Loopback names are always accepted for the configured port. A concrete
/// non-loopback host adds exactly itself. A wildcard bind accepts the authority
/// the browser used (`Host`), which keeps same-origin requests working without
/// accepting an unrelated site's origin. `--host` defaults to loopback, so the
/// `Host` header only widens the allowlist after an explicit wildcard bind.
fn origin_allowed(
    origin: &str,
    configured_host: &str,
    port: u16,
    request_host: Option<&str>,
) -> bool {
    if origin == format!("http://{}", authority("127.0.0.1", port))
        || origin == format!("http://{}", authority("localhost", port))
        || origin == format!("http://{}", authority("::1", port))
    {
        return true;
    }
    if is_wildcard(configured_host) {
        return request_host.is_some_and(|host| origin == format!("http://{host}"));
    }
    if is_loopback(configured_host) {
        return false;
    }
    origin == format!("http://{}", authority(configured_host, port))
}

async fn check_origin(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path().starts_with("/api") {
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let host_header = request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok());
            if !origin_allowed(origin, &state.options.host, state.options.port, host_header) {
                return (StatusCode::FORBIDDEN, "cross-origin request refused").into_response();
            }
        }
        if request
            .headers()
            .get("sec-fetch-site")
            .and_then(|value| value.to_str().ok())
            == Some("cross-site")
        {
            return (StatusCode::FORBIDDEN, "cross-site request refused").into_response();
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    // The document is local-first: no external scripts, styles, fonts or
    // images, and no framing. `wasm-unsafe-eval` is required for the engine.
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; font-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    // `trunk` writes fixed asset names; revalidate so a rebuilt UI is never
    // masked by a stale wasm module. API responses already say `no-store`.
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-cache"));
    response
}

/// Resolve a project-relative path and refuse anything that escapes the root.
pub fn safe_join(root: &Path, src: &str) -> Option<PathBuf> {
    let path = Path::new(src);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let joined = root.join(path);
    let canonical = joined.canonicalize().ok()?;
    canonical.starts_with(root).then_some(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_allowed_keeps_the_default_loopback_only() {
        assert!(origin_allowed(
            "http://127.0.0.1:7070",
            "127.0.0.1",
            7070,
            None
        ));
        assert!(origin_allowed(
            "http://localhost:7070",
            "127.0.0.1",
            7070,
            None
        ));
        assert!(origin_allowed("http://[::1]:7070", "127.0.0.1", 7070, None));
        assert!(!origin_allowed(
            "http://10.0.0.6:7070",
            "127.0.0.1",
            7070,
            Some("10.0.0.6:7070")
        ));
        assert!(!origin_allowed(
            "http://127.0.0.1:7070",
            "127.0.0.1",
            7091,
            None
        ));
    }

    #[test]
    fn origin_allowed_adds_a_concrete_lan_host() {
        assert!(origin_allowed(
            "http://10.0.0.6:7070",
            "10.0.0.6",
            7070,
            None
        ));
        assert!(origin_allowed(
            "http://127.0.0.1:7070",
            "10.0.0.6",
            7070,
            None
        ));
        assert!(!origin_allowed(
            "http://10.0.0.7:7070",
            "10.0.0.6",
            7070,
            Some("10.0.0.7:7070")
        ));
        assert!(!origin_allowed(
            "http://evil.example:7070",
            "10.0.0.6",
            7070,
            None
        ));
    }

    #[test]
    fn origin_allowed_wildcard_follows_the_request_host() {
        assert!(origin_allowed(
            "http://10.0.0.6:7070",
            "0.0.0.0",
            7070,
            Some("10.0.0.6:7070")
        ));
        assert!(!origin_allowed(
            "http://evil.example:7070",
            "0.0.0.0",
            7070,
            Some("10.0.0.6:7070")
        ));
        assert!(!origin_allowed(
            "http://10.0.0.6:7070",
            "0.0.0.0",
            7070,
            None
        ));
    }

    #[test]
    fn origin_allowed_brackets_ipv6_hosts() {
        assert_eq!(authority("::1", 7070), "[::1]:7070");
        assert_eq!(authority("[::1]", 7070), "[::1]:7070");
        assert_eq!(authority("10.0.0.6", 7070), "10.0.0.6:7070");
        assert!(origin_allowed("http://[::1]:7070", "::1", 7070, None));
        assert!(origin_allowed(
            "http://[fd00::6]:7070",
            "fd00::6",
            7070,
            None
        ));
    }

    #[test]
    fn parse_args_reads_the_host_flag() {
        let input = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        let args = vec![
            String::from("--input"),
            String::from(input),
            String::from("--host"),
            String::from("10.0.0.6"),
        ];
        let (options, _) = parse_args(args).expect("args should parse");
        assert_eq!(options.host, "10.0.0.6");
        assert_eq!(options.port, 7070);

        let args = vec![
            String::from("--input"),
            String::from(input),
            String::from("--host"),
            String::new(),
        ];
        assert!(parse_args(args).is_err());
    }
}
