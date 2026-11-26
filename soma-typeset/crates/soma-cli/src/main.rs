//! `soma` — one command from a source document to rendered output.
//!
//! ```text
//! soma render --input doc.tex --template book.yaml --format html --out doc.html
//! soma render --input doc.md  --format svg
//! soma render --input doc.md  --format json
//! soma check  --input doc.md  --template book.yaml
//! ```
//!
//! The CLI owns only I/O and the choice of frontend/renderer. Parsing, styling,
//! layout and rendering are the library crates; nothing here re-implements
//! typesetting, and the output is the same canonical result the gates hash.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use soma_core::{BreakConfig, StyleId, VerticalParams};
use soma_doc::Document;
use soma_engine::EngineConfig;
use soma_layout::{BoxKind, PaginationStrategy, StyledMetrics, TextSource};
use soma_metrics::MetricsTable;
use soma_render_pdf::{render as render_pdf, ImageSource, PdfFont, PdfFonts, PdfMetadata};
use soma_template::{parse_template, resolve, StyledDoc, Template};

const USAGE: &str = "\
soma — deterministic Chinese-first typesetting

usage:
  soma render --input <file|dir|-> [--from md|html|tex|docx]
              [--template <file>] [--engine <file>]
              [--format html|svg|json|pdf] [--out <path>]
              [--metrics <file>]
              [--font <file[#index]>] [--font-bold <file[#index]>]
              [--font-sans <file[#index]>] [--font-sans-bold <file[#index]>]
              [--font-mono <file[#index]>] [--font-mono-bold <file[#index]>]
              [--font-math <file[#index]>] [--font-fallback <file[#index]>]
  soma check  --input <file|-> [--from md|html|tex|docx]
              [--template <file>] [--engine <file>]
  soma diagram --input <file|-> [--from mermaid|dot]
              [--template <file>] [--metrics <file>] [--out <path>]
  soma --version

  --input     .md / .markdown, .html / .htm, .tex / .latex, .docx;
              a directory (batch) or '-' (stdin)
  --from      source format for '-', or to override a file extension
  --template  pure-YAML template; omitted uses the built-in Plex design
  --engine    soma.engine/1 configuration; omitted uses latex-v1
  --format    html (default), svg, json (canonical page result), or pdf
  --metrics   soma.metrics table; omitted uses the embedded table.
              Generate one with tools/metrics-gen to match a PDF font.
  --font      TrueType font for PDF output (required by --format pdf)
  --font-bold optional bold face; falls back to --font
  --font-sans optional sans face; falls back to --font
  --font-sans-bold optional bold sans face
  --font-mono optional monospaced face; falls back to --font-sans, then --font
  --font-mono-bold optional bold monospaced face
  --font-math optional math face; omitted uses the bundled New Computer
              Modern Math the embedded table was measured from, then probes
              installed faces, then falls back to --font with a warning
  --font-fallback optional rescue face for glyphs --font lacks
  --hyphenation TeX pattern file enabling hyphenation of long Latin words
  --stats     print pages, blocks, lines, canonical hash (and the chosen
              math face for PDF) to stderr
  --out       output path; '-' or omitted writes to stdout.
              With --input <dir> this must be a directory: every supported
              file directly inside it writes <stem>.<format-extension> there.
              Batch mode is non-recursive and one-shot; there is no --watch.

exit codes:
  0  success
  1  usage, input, template, engine, or layout error
";

/// The built-in design: the Plex reference page (ADR-0088). It is the same
/// file `scripts/plex.sh` renders, embedded so a single binary still works.
const DEFAULT_TEMPLATE: &str = include_str!("../../../examples/templates/plex.yaml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Html,
    Svg,
    Json,
    Pdf,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Svg => "svg",
            Self::Json => "json",
            Self::Pdf => "pdf",
        }
    }
}

/// The frontend, fixed by extension or by `--from` for a stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceFormat {
    Markdown,
    Html,
    Latex,
    Docx,
}

impl SourceFormat {
    fn from_cli(value: &str) -> Option<Self> {
        match value {
            "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "tex" | "latex" => Some(Self::Latex),
            "docx" => Some(Self::Docx),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct Options {
    input: String,
    from: Option<SourceFormat>,
    template: Option<String>,
    engine: Option<String>,
    format: Format,
    out: Option<String>,
    stats: bool,
    metrics: Option<String>,
    font: Option<String>,
    font_bold: Option<String>,
    font_sans: Option<String>,
    font_sans_bold: Option<String>,
    font_mono: Option<String>,
    font_mono_bold: Option<String>,
    font_math: Option<String>,
    font_fallback: Option<String>,
    hyphenation: Option<String>,
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("soma: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some(command) = args.first() else {
        return Err(format!("missing command\n\n{USAGE}"));
    };
    match command.as_str() {
        "render" => render(&args[1..]),
        "check" => check(&args[1..]),
        "diagram" => diagram(&args[1..]),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("soma {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn parse_options(args: &[String], allow_format: bool) -> Result<Options, String> {
    let mut input: Option<String> = None;
    let mut from = None;
    let mut template = None;
    let mut engine = None;
    let mut format = Format::Html;
    let mut out = None;
    let mut stats = false;
    let mut metrics = None;
    let mut font = None;
    let mut font_bold = None;
    let mut font_sans = None;
    let mut font_sans_bold = None;
    let mut font_mono = None;
    let mut font_mono_bold = None;
    let mut font_math = None;
    let mut font_fallback = None;
    let mut hyphenation = None;

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
            "--input" | "-i" => input = Some(value()?),
            "--from" => {
                let name = value()?;
                from = Some(SourceFormat::from_cli(&name).ok_or_else(|| {
                    format!("unknown source format `{name}` (expected md, html, tex, or docx)")
                })?);
            }
            "--template" | "-t" => template = Some(value()?),
            "--engine" => engine = Some(value()?),
            "--out" | "-o" => out = Some(value()?),
            "--metrics" => metrics = Some(value()?),
            "--font" => font = Some(value()?),
            "--font-bold" => font_bold = Some(value()?),
            "--font-sans" => font_sans = Some(value()?),
            "--font-sans-bold" => font_sans_bold = Some(value()?),
            "--font-mono" => font_mono = Some(value()?),
            "--font-mono-bold" => font_mono_bold = Some(value()?),
            "--font-math" => font_math = Some(value()?),
            "--font-fallback" => font_fallback = Some(value()?),
            "--hyphenation" => hyphenation = Some(value()?),
            "--stats" => stats = true,
            "--format" | "-f" if allow_format => {
                format = match value()?.as_str() {
                    "html" => Format::Html,
                    "svg" => Format::Svg,
                    "json" => Format::Json,
                    "pdf" => Format::Pdf,
                    other => return Err(format!("unknown format `{other}`")),
                }
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
        index += 1;
    }

    Ok(Options {
        input: input.ok_or_else(|| format!("--input is required\n\n{USAGE}"))?,
        from,
        template,
        engine,
        format,
        out,
        stats,
        metrics,
        font,
        font_bold,
        font_sans,
        font_sans_bold,
        font_mono,
        font_mono_bold,
        font_math,
        font_fallback,
        hyphenation,
    })
}

fn render(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, true)?;
    if options.input != "-" && Path::new(&options.input).is_dir() {
        return render_directory(&options);
    }
    render_one(&options, None)
}

fn render_one(options: &Options, label: Option<&str>) -> Result<(), String> {
    let pipeline = pipeline(options)?;
    let metrics = load_metrics(options)?;
    let pages = layout(
        &pipeline.document,
        &pipeline.styled,
        &pipeline.engine,
        pipeline.vertical,
        &metrics,
        options,
    )?;
    let styled = &pipeline.styled;
    let document = &pipeline.document;
    let assets = FileImages::load(&options.input, &pages).with_embedded(&pipeline.media);
    match options.format {
        Format::Html => write_output(
            options,
            &soma_render_html::render_with(&pages, styled.table(), &pages.text, &assets),
        )?,
        Format::Svg => write_output(
            options,
            &soma_render_svg::render_with(&pages, styled.table(), &pages.text, &assets),
        )?,
        Format::Json => {
            let mut json = pages.canonical_json();
            json.push('\n');
            write_output(options, &json)?;
        }
        Format::Pdf => {
            let needs_math = pages_have_math(&pages);
            let faces = load_pdf_fonts(options, &metrics, needs_math)?;
            let title = document.title_text();
            let author = {
                let names: Vec<&str> = document
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
            let metadata = PdfMetadata {
                title: title.as_deref(),
                author: author.as_deref(),
                language: document.meta.language.as_deref(),
            };
            let fonts = PdfFonts {
                regular: &faces.regular,
                bold: faces.bold.as_ref(),
                sans: faces.sans.as_ref(),
                sans_bold: faces.sans_bold.as_ref(),
                mono: faces.mono.as_ref(),
                mono_bold: faces.mono_bold.as_ref(),
                math: faces.math.as_ref(),
                fallback: faces.fallback.as_ref(),
            };
            let bytes = render_pdf(&pages, styled, &pages.text, &fonts, &metadata, &assets)
                .map_err(|error| format!("pdf: {error}"))?;
            write_bytes(options, &bytes)?;
            if options.stats {
                match &faces.math_source {
                    MathSource::Explicit => eprintln!(
                        "math: {} (explicit)",
                        options.font_math.as_deref().unwrap_or("")
                    ),
                    MathSource::Auto(spec) => eprintln!("math: {spec} (auto)"),
                    MathSource::None => eprintln!("math: none"),
                }
            }
            if needs_math && faces.math.is_none() {
                eprintln!(
                    "soma: warning: no installed math face matches the metrics table; \
                     formula symbols use the body face (pass --font-math <file>)"
                );
            }
        }
    }
    if options.stats {
        print_stats(label, &pages);
    }
    Ok(())
}

/// Render every supported file directly inside `--input` into `--out`.
///
/// Batch mode is deliberately one-shot and non-recursive: no watcher, no
/// traversal, and no output path the user did not give. Every target is
/// checked against the input set before the first write, so a collision
/// (for example `a.html` read and `a.md` written as `a.html` in the same
/// directory) is an error instead of a silent overwrite.
fn render_directory(options: &Options) -> Result<(), String> {
    if options.from.is_some() {
        return Err(String::from(
            "--from applies to one input, not to a directory",
        ));
    }
    let out_argument = options.out.as_deref().filter(|path| *path != "-");
    let Some(out_argument) = out_argument else {
        return Err(format!(
            "{}: --input <dir> needs --out <dir> (batch mode never overwrites sources)",
            options.input
        ));
    };
    let out_dir = PathBuf::from(out_argument);
    if out_dir.exists() && !out_dir.is_dir() {
        return Err(format!(
            "{out_argument}: --out must be a directory for --input <dir>"
        ));
    }

    let entries =
        std::fs::read_dir(&options.input).map_err(|error| format!("{}: {error}", options.input))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", options.input))?;
        let path = entry.path();
        let is_file = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", path.display()))?
            .is_file();
        if is_file && supported_input(&path) {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "{}: no supported files (md, html, tex, docx) in the directory",
            options.input
        ));
    }

    std::fs::create_dir_all(&out_dir).map_err(|error| format!("{out_argument}: {error}"))?;
    let out_dir =
        std::fs::canonicalize(&out_dir).map_err(|error| format!("{out_argument}: {error}"))?;
    let inputs: BTreeSet<PathBuf> = files
        .iter()
        .filter_map(|file| std::fs::canonicalize(file).ok())
        .collect();

    let mut jobs = Vec::with_capacity(files.len());
    let mut targets = BTreeSet::new();
    for file in &files {
        let stem = file
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| format!("{}: file stem is not UTF-8", file.display()))?;
        let target = out_dir.join(format!("{stem}.{}", options.format.extension()));
        let canonical = std::fs::canonicalize(&target).unwrap_or_else(|_| {
            out_dir.join(target.file_name().expect("the target has a file name"))
        });
        if inputs.contains(&canonical) {
            return Err(format!(
                "{}: output would overwrite an input file; choose a different --out directory",
                target.display()
            ));
        }
        if !targets.insert(canonical) {
            return Err(format!(
                "{}: two input files share this output path",
                target.display()
            ));
        }
        jobs.push((file.clone(), target));
    }

    for (file, target) in jobs {
        let label = file
            .to_str()
            .ok_or_else(|| format!("{}: file name is not UTF-8", file.display()))?;
        let mut single = options.clone();
        single.input = label.to_string();
        single.out = Some(
            target
                .to_str()
                .ok_or_else(|| format!("{}: path is not UTF-8", target.display()))?
                .to_string(),
        );
        render_one(&single, Some(label))?;
    }
    Ok(())
}

fn supported_input(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "md" | "markdown" | "html" | "htm" | "tex" | "latex" | "docx"
    )
}

fn print_stats(label: Option<&str>, pages: &soma_layout::PagedLayout) {
    let line = format!(
        "pages={} blocks={} lines={} fallbacks={} canonicalHash={:016x}",
        pages.page_count(),
        pages
            .pages
            .iter()
            .map(|page| page.blocks.len())
            .sum::<usize>(),
        pages
            .pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .map(|block| block.lines_total)
            .sum::<u32>(),
        pages.text.fallback_count(),
        pages.hash()
    );
    match label {
        Some(label) => eprintln!("{label}: {line}"),
        None => eprintln!("{line}"),
    }
}

fn diagram(args: &[String]) -> Result<(), String> {
    let mut input: Option<String> = None;
    let mut from: Option<String> = None;
    let mut template_path: Option<String> = None;
    let mut metrics_path: Option<String> = None;
    let mut out: Option<String> = None;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg {
            "--input" | "-i" => input = Some(value()?),
            "--from" => from = Some(value()?),
            "--template" | "-t" => template_path = Some(value()?),
            "--metrics" => metrics_path = Some(value()?),
            "--out" => out = Some(value()?),
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown diagram argument `{other}`")),
        }
        index += 1;
    }
    let input = input.ok_or_else(|| String::from("diagram needs --input"))?;
    let source = if input == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| format!("stdin: {error}"))?;
        text
    } else {
        std::fs::read_to_string(&input).map_err(|error| format!("{input}: {error}"))?
    };
    let dialect = match from.as_deref() {
        Some("mermaid") | Some("mmd") | Some("flowchart") => soma_doc::DiagramDialect::Mermaid,
        Some("dot") | Some("graphviz") => soma_doc::DiagramDialect::Graphviz,
        Some(other) => return Err(format!("unknown diagram dialect `{other}`")),
        None if input.ends_with(".dot") || input.ends_with(".gv") => {
            soma_doc::DiagramDialect::Graphviz
        }
        None => soma_doc::DiagramDialect::Mermaid,
    };
    let template_text = match template_path {
        Some(path) => std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?,
        None => DEFAULT_TEMPLATE.to_string(),
    };
    let template = parse_template(&template_text).map_err(|error| error.to_string())?;
    let document = Document::default();
    let styled = resolve(&document, &template).map_err(|error| error.to_string())?;
    let table = match metrics_path {
        Some(path) => {
            let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
            MetricsTable::parse(&bytes).map_err(|error| error.to_string())?
        }
        None => MetricsTable::embedded(),
    };
    let styled_metrics = soma_layout::StyledMetrics::new(&table, &styled);
    let style = styled
        .style(StyleId(0))
        .cloned()
        .unwrap_or_else(soma_template::ComputedStyle::base);
    let options = soma_diagram::Options {
        label_size: style.font_size,
        line_gap: style.line_height,
        line_width: soma_core::Width::from_em_frac(1, 25),
        max_label_width: style.font_size.scale(16, 1),
    };
    let mut text = soma_layout::DiagramText::new(&style, &styled_metrics, StyleId(0));
    let layout = soma_diagram::layout(dialect, &source, &options, &mut text)
        .map_err(|error| error.to_string())?;
    let tikz = soma_diagram::to_tikz(&layout);
    match out {
        Some(path) if path != "-" => {
            std::fs::write(&path, tikz).map_err(|error| format!("{path}: {error}"))?
        }
        _ => print!("{tikz}"),
    }
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, false)?;
    if options.input != "-" && Path::new(&options.input).is_dir() {
        return Err(format!(
            "{}: check expects one file; directory batch mode is render-only",
            options.input
        ));
    }
    let pipeline = pipeline(&options)?;
    let metrics = load_metrics(&options)?;
    let pages = layout(
        &pipeline.document,
        &pipeline.styled,
        &pipeline.engine,
        pipeline.vertical,
        &metrics,
        &options,
    )?;
    let document = &pipeline.document;
    let overflow = pages.pages.iter().filter(|page| page.overflow).count();
    let emergency = pages
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .filter(|block| block.stage != soma_core::FallbackStage::Normal)
        .count();

    println!("source: {}", options.input);
    println!(
        "language: {}",
        document.meta.language.as_deref().unwrap_or("-")
    );
    println!("blocks: {}", document.blocks.len());
    println!("pages: {}", pages.page_count());
    println!("overflow pages: {overflow}");
    println!("non-normal blocks: {emergency}");
    println!("canonical hash: {:016x}", pages.hash());
    Ok(())
}

struct Pipeline {
    document: Document,
    styled: StyledDoc,
    engine: BreakConfig,
    vertical: VerticalParams,
    /// Images that came from inside a container (a `.docx` archive).
    media: Vec<soma_docx::MediaFile>,
}

fn pipeline(options: &Options) -> Result<Pipeline, String> {
    let format = source_format(&options.input, options.from)?;
    let (mut document, media) = read_document(&options.input, format)?;

    enrich_image_sizes(&mut document, &options.input);

    let template_source = match &options.template {
        Some(path) => std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?,
        None => DEFAULT_TEMPLATE.to_string(),
    };
    let template: Template =
        parse_template(&template_source).map_err(|error| format!("template: {error}"))?;
    let styled = resolve(&document, &template).map_err(|error| format!("template: {error}"))?;

    let (engine, vertical) = match &options.engine {
        Some(path) => {
            let source =
                std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
            let config = EngineConfig::parse(&source).map_err(|error| error.to_string())?;
            (config.break_config(), config.vertical())
        }
        // The CLI default follows LaTeX article: `\tolerance=200` (badness),
        // no `\emergencystretch`, and a ragged last line. An explicit
        // `--engine` file still selects its own preset.
        None => (
            BreakConfig::from_params(soma_core::ParamSet::latex_v1()),
            VerticalParams::default_v1(),
        ),
    };
    Ok(Pipeline {
        document,
        styled,
        engine,
        vertical,
        media,
    })
}

/// The frontend comes from `--from` when given, otherwise from the extension.
/// A stream has no name to inspect, so `--input -` without `--from` is an
/// error rather than a guess (ADR-0029).
fn source_format(input: &str, forced: Option<SourceFormat>) -> Result<SourceFormat, String> {
    if let Some(format) = forced {
        return Ok(format);
    }
    if input == "-" {
        return Err(String::from(
            "--input - needs --from md|html|tex|docx (a stream has no extension)",
        ));
    }
    let extension = Path::new(input)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "md" | "markdown" => Ok(SourceFormat::Markdown),
        "html" | "htm" => Ok(SourceFormat::Html),
        "tex" | "latex" => Ok(SourceFormat::Latex),
        "docx" => Ok(SourceFormat::Docx),
        other => Err(format!(
            "{input}: unsupported input `{other}` (expected md, html, tex, or docx)"
        )),
    }
}

/// Read the source bytes. Stdin is read as bytes so `.docx` works there too;
/// text frontends then require valid UTF-8.
fn read_document(
    input: &str,
    format: SourceFormat,
) -> Result<(Document, Vec<soma_docx::MediaFile>), String> {
    let label = input_label(input);
    let bytes = if input == "-" {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("stdin: {error}"))?;
        bytes
    } else {
        std::fs::read(input).map_err(|error| format!("{label}: {error}"))?
    };
    if format == SourceFormat::Docx {
        return soma_docx::parse_docx_with_media(&bytes)
            .map_err(|error| format!("{label}: {}", error.reason));
    }
    let source = String::from_utf8(bytes)
        .map_err(|error| format!("{label}: input is not valid UTF-8 ({})", error.utf8_error()))?;
    let document = match format {
        SourceFormat::Markdown => soma_md::parse_markdown(&source),
        SourceFormat::Html => soma_html::parse_html(&source),
        SourceFormat::Latex => soma_latex::parse_latex(&source),
        SourceFormat::Docx => unreachable!("handled above"),
    };
    Ok((document, Vec::new()))
}

fn input_label(input: &str) -> &str {
    if input == "-" {
        "<stdin>"
    } else {
        input
    }
}

/// Images sit next to the input file. A stream has no directory, so the
/// current directory is the only sensible base.
fn input_base(input: &str) -> PathBuf {
    if input == "-" {
        PathBuf::from(".")
    } else {
        Path::new(input)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

/// Read the intrinsic pixel size of each local image so the layout knows its
/// aspect ratio. DocIR deliberately carries no width, but an image's own size
/// is a fact about the asset, not a style (the same rule the Word importer
/// follows). A missing or unsupported file is left alone: the layout keeps its
/// aspect fallback.
fn enrich_image_sizes(document: &mut Document, input: &str) {
    let base = input_base(input);
    enrich_blocks(&mut document.blocks, &base);
    for footnote in &mut document.footnotes {
        enrich_blocks(&mut footnote.blocks, &base);
    }
}

fn enrich_blocks(blocks: &mut [soma_doc::Block], base: &std::path::Path) {
    for block in blocks {
        match block {
            soma_doc::Block::Figure(node) => enrich_image(&mut node.image.source, base),
            soma_doc::Block::Paragraph(node) => enrich_inlines(&mut node.inlines, base),
            soma_doc::Block::Heading(node) => enrich_inlines(&mut node.inlines, base),
            soma_doc::Block::Table(node) => {
                enrich_inlines(&mut node.caption, base);
                for row in &mut node.rows {
                    for cell in &mut row.cells {
                        enrich_blocks(&mut cell.blocks, base);
                    }
                }
            }
            soma_doc::Block::List(node) => {
                for item in &mut node.items {
                    enrich_blocks(&mut item.blocks, base);
                }
            }
            soma_doc::Block::BlockQuote(node) => enrich_blocks(&mut node.blocks, base),
            soma_doc::Block::Generated(node) => enrich_blocks(&mut node.blocks, base),
            _ => {}
        }
    }
}

fn enrich_inlines(inlines: &mut [soma_doc::Inline], base: &std::path::Path) {
    for inline in inlines {
        match inline {
            soma_doc::Inline::Image(node) => enrich_image(&mut node.source, base),
            soma_doc::Inline::Strong(node)
            | soma_doc::Inline::Emphasis(node)
            | soma_doc::Inline::Underline(node)
            | soma_doc::Inline::Strikethrough(node)
            | soma_doc::Inline::SmallCaps(node)
            | soma_doc::Inline::Superscript(node)
            | soma_doc::Inline::Subscript(node) => enrich_inlines(&mut node.children, base),
            soma_doc::Inline::Link(node) => enrich_inlines(&mut node.children, base),
            _ => {}
        }
    }
}

fn enrich_image(image: &mut soma_doc::ImageRef, base: &std::path::Path) {
    if image.intrinsic.is_some() || image.src.contains("://") || image.src.starts_with("data:") {
        return;
    }
    let Ok(bytes) = std::fs::read(base.join(&image.src)) else {
        return;
    };
    if let Some((width_px, height_px)) = image_size(&bytes) {
        image.intrinsic = Some(soma_doc::ImageIntrinsic {
            width_px,
            height_px,
        });
    }
}

/// PNG IHDR or a JPEG start-of-frame, in pixels.
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2usize;
    while at + 9 <= bytes.len() {
        if bytes[at] != 0xFF {
            at += 1;
            continue;
        }
        let marker = bytes[at + 1];
        // Standalone markers carry no length.
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 || marker == 0xFF {
            at += 2;
            continue;
        }
        let length = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        if length < 2 {
            return None;
        }
        let frame = matches!(
            marker,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF
        );
        if frame && at + 9 <= bytes.len() {
            let height = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]) as u32;
            return (width > 0 && height > 0).then_some((width, height));
        }
        at += 2 + length;
    }
    None
}

fn load_metrics(options: &Options) -> Result<MetricsTable, String> {
    match options.metrics.as_deref() {
        None => Ok(MetricsTable::embedded()),
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
            MetricsTable::parse(&bytes).map_err(|error| format!("{path}: {error}"))
        }
    }
}

fn layout(
    document: &Document,
    styled: &StyledDoc,
    engine: &BreakConfig,
    vertical: VerticalParams,
    table: &MetricsTable,
    options: &Options,
) -> Result<soma_layout::PagedLayout, String> {
    let metrics = StyledMetrics::new(table, styled);
    let patterns = match options.hyphenation.as_deref() {
        Some(path) => Some(soma_hyphen::Patterns::parse(
            &std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?,
        )),
        None => None,
    };
    soma_layout::layout_with_hyphenation(
        document,
        styled,
        &metrics,
        engine,
        PaginationStrategy::VerticalKp,
        vertical,
        patterns.as_ref(),
    )
    .map_err(|error| error.to_string())
}

/// Load the fonts requested by `--format pdf`.
/// The four faces a PDF can embed: body, bold, math and a rescue face.
struct PdfFaceSet {
    regular: PdfFont,
    bold: Option<PdfFont>,
    sans: Option<PdfFont>,
    sans_bold: Option<PdfFont>,
    mono: Option<PdfFont>,
    mono_bold: Option<PdfFont>,
    math: Option<PdfFont>,
    fallback: Option<PdfFont>,
    /// How the math face was chosen, for `--stats` and the warning.
    math_source: MathSource,
}

enum MathSource {
    /// `--font-math` was given; trusted as-is.
    Explicit,
    /// Probed, and its advances agree with the metrics table.
    Auto(&'static str),
    /// No math face: formula symbols are drawn with the body face.
    None,
}

/// The embedded metrics table was measured from New Computer Modern Math,
/// the face Typst 0.13 uses for formulas. Bundling the same face makes the
/// default machine-independent (ADR-0079).
const EMBEDDED_MATH_FACE: &[u8] = include_bytes!("../assets/NewCMMath-Regular.otf");

/// Installed math faces to try when `--font-math` is omitted, in preference
/// order. A candidate is used only when its advances agree with the metrics
/// table: a different math face resolves the same ordinals but draws variants
/// at other sizes and places limits from other ink (ADR-0077).
const MATH_FACE_CANDIDATES: &[&str] = &[
    "/usr/share/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf",
    "/usr/share/texmf/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf",
    "/usr/share/texlive/texmf-dist/fonts/opentype/public/newcomputermodern/NewCMMath-Regular.otf",
    "/usr/share/fonts/OTF/NewCMMath-Regular.otf",
    "/usr/share/fonts/newcomputermodern/NewCMMath-Regular.otf",
    "/usr/share/texmf-dist/fonts/opentype/public/lm-math/latinmodern-math.otf",
    "/usr/share/texmf/fonts/opentype/public/lm-math/latinmodern-math.otf",
    "/usr/share/texlive/texmf-dist/fonts/opentype/public/lm-math/latinmodern-math.otf",
    "/usr/share/fonts/OTF/latinmodern-math.otf",
    "/usr/share/fonts/lm-math/latinmodern-math.otf",
    "/usr/share/fonts/noto/NotoSansMath-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMath-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansMath-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSansMath-Regular.ttf",
    "/usr/local/share/fonts/NotoSansMath-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuMathTeXGyre.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuMathTeXGyre.ttf",
    "/usr/share/fonts/dejavu/DejaVuMathTeXGyre.ttf",
    "/usr/share/fonts/OTF/texgyrepagella-math.otf",
    "/usr/share/fonts/tex-gyre-math/texgyrepagella-math.otf",
    "/usr/share/texmf/fonts/opentype/public/tex-gyre-math/texgyrepagella-math.otf",
    "/usr/share/texlive/texmf-dist/fonts/opentype/public/tex-gyre-math/texgyrepagella-math.otf",
    "/usr/share/fonts/OTF/LibertinusMath-Regular.otf",
    "/usr/share/fonts/libertinus/LibertinusMath-Regular.otf",
    "/System/Library/Fonts/Supplemental/STIXTwoMath.otf",
    "/Library/Fonts/STIXTwoMath.otf",
    "C:\\Windows\\Fonts\\STIXTwoMath.otf",
    "C:\\Windows\\Fonts\\cambria.ttc#1",
];

fn load_pdf_fonts(
    options: &Options,
    table: &MetricsTable,
    needs_math: bool,
) -> Result<PdfFaceSet, String> {
    let regular = options
        .font
        .as_deref()
        .ok_or_else(|| String::from("--format pdf needs --font <path[#index]>"))?;
    let regular = load_pdf_font(regular)?;
    let bold = match options.font_bold.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    let sans = match options.font_sans.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    let sans_bold = match options.font_sans_bold.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    let mono = match options.font_mono.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    let mono_bold = match options.font_mono_bold.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    let (math, math_source) = match options.font_math.as_deref() {
        Some(spec) => (Some(load_pdf_font(spec)?), MathSource::Explicit),
        None if needs_math => match installed_math_face(table) {
            Some((font, spec)) => (Some(font), MathSource::Auto(spec)),
            None => (None, MathSource::None),
        },
        None => (None, MathSource::None),
    };
    let fallback = match options.font_fallback.as_deref() {
        Some(spec) => Some(load_pdf_font(spec)?),
        None => None,
    };
    Ok(PdfFaceSet {
        regular,
        bold,
        sans,
        sans_bold,
        mono,
        mono_bold,
        math,
        fallback,
        math_source,
    })
}

/// Whether any block on any page carries a formula. The probe reads font files,
/// so a text-only document never pays for it.
fn pages_have_math(pages: &soma_layout::PagedLayout) -> bool {
    pages
        .pages
        .iter()
        .flat_map(|page| page.blocks.iter())
        .any(|block| block.math.is_some() || !block.inline_math.is_empty())
}

fn installed_math_face(table: &MetricsTable) -> Option<(PdfFont, &'static str)> {
    if let Some(font) = embedded_math_face(table) {
        return Some((font, "built-in New Computer Modern Math"));
    }
    MATH_FACE_CANDIDATES.iter().find_map(|spec| {
        let (path, index) = split_font_spec(spec).ok()?;
        let bytes = std::fs::read(path).ok()?;
        let font = PdfFont::from_bytes(bytes, index).ok()?;
        math_face_matches(table, &font).then_some((font, *spec))
    })
}

/// The bundled face, accepted only when it is the one the table measured.
fn embedded_math_face(table: &MetricsTable) -> Option<PdfFont> {
    let font = PdfFont::from_bytes(EMBEDDED_MATH_FACE.to_vec(), 0).ok()?;
    math_face_matches(table, &font).then_some(font)
}

/// Whether `font` is the face `table`'s math metrics came from.
///
/// The table carries numbers, not a font name; `Face::math_matches` samples its
/// math glyph advances and compares them in the table's own unit (1/4096 em).
fn math_face_matches(table: &MetricsTable, font: &PdfFont) -> bool {
    table
        .math_face()
        .is_some_and(|face| face.math_matches(|codepoint| font.advance_4096(codepoint)))
}

fn load_pdf_font(spec: &str) -> Result<PdfFont, String> {
    let (path, index) = split_font_spec(spec)?;
    let bytes = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    PdfFont::from_bytes(bytes, index).map_err(|error| format!("{path}: {error}"))
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

/// Image assets that sit next to the input document.
///
/// Only local paths are read: a source string with a scheme (`http:`, `data:`,
/// …) is ignored, so a document can never make the renderer talk to the
/// network. Anything that fails to load leaves the figure as a placeholder
/// rather than failing the run (ADR-0047).
struct FileImages {
    files: std::collections::BTreeMap<String, Vec<u8>>,
}

impl FileImages {
    fn load(input: &str, pages: &soma_layout::PagedLayout) -> Self {
        let base = input_base(input);
        let mut files = std::collections::BTreeMap::new();
        for page in &pages.pages {
            for block in &page.blocks {
                if block.kind != BoxKind::Figure {
                    continue;
                }
                let Some(src) = pages.text.block_image(block.node) else {
                    continue;
                };
                if files.contains_key(src) || src.contains("://") || src.starts_with("data:") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(base.join(src)) {
                    files.insert(String::from(src), bytes);
                }
            }
        }
        Self { files }
    }
}

impl FileImages {
    /// Images that travelled inside the input (a `.docx` archive) are keyed by
    /// the same name the DocIR uses, so the renderers need no special case.
    /// A file next to the input wins: it lets an author override an embedded
    /// asset without editing the archive.
    fn with_embedded(mut self, media: &[soma_docx::MediaFile]) -> Self {
        for file in media {
            self.files
                .entry(file.name.clone())
                .or_insert_with(|| file.bytes.clone());
        }
        self
    }
}

impl ImageSource for FileImages {
    fn image(&self, src: &str) -> Option<&[u8]> {
        self.files.get(src).map(|bytes| bytes.as_slice())
    }
}

fn write_bytes(options: &Options, output: &[u8]) -> Result<(), String> {
    match options.out.as_deref() {
        None | Some("-") => {
            use std::io::Write;
            std::io::stdout()
                .write_all(output)
                .map_err(|error| format!("stdout: {error}"))
        }
        Some(path) => std::fs::write(path, output).map_err(|error| format!("{path}: {error}")),
    }
}

fn write_output(options: &Options, output: &str) -> Result<(), String> {
    match options.out.as_deref() {
        None | Some("-") => {
            print!("{output}");
            Ok(())
        }
        Some(path) => std::fs::write(path, output).map_err(|error| format!("{path}: {error}")),
    }
}
