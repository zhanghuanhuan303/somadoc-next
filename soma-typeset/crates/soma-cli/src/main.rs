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

use std::process::ExitCode;

use soma_core::{BreakConfig, VerticalParams};
use soma_doc::Document;
use soma_engine::EngineConfig;
use soma_layout::{layout_with, PaginationStrategy, StyledMetrics};
use soma_metrics::MetricsTable;
use soma_render_html::render as render_html;
use soma_render_svg::render as render_svg;
use soma_template::{parse_template, resolve, StyledDoc, Template};

const USAGE: &str = "\
soma — deterministic Chinese-first typesetting

usage:
  soma render --input <file> [--template <file>] [--engine <file>]
              [--format html|svg|json] [--out <file>]
  soma check  --input <file> [--template <file>] [--engine <file>]

  --input     .md / .markdown, .html / .htm, .tex / .latex
  --template  pure-YAML template; omitted uses the built-in A4 default
  --engine    soma.engine/1 configuration; omitted uses publish-v1
  --format    html (default), svg, or json (canonical page result)
  --out       output path; '-' or omitted writes to stdout

exit codes:
  0  success
  1  usage, input, template, engine, or layout error
";

/// A4, 20 mm margins, justified 10.5 pt body. Mirrors `soma_eval`'s template so
/// `soma render` works before a designer writes one.
const DEFAULT_TEMPLATE: &str = r#"
name: soma-default
version: 1
page:
  size: A4
  margin: 20mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: paragraph
    declarations:
      first-indent: 2em
  - selector:
      element: heading
    declarations:
      space-before: 1.2em
      space-after: 0.4em
      keep-with-next: true
  - selector:
      element: footnote
    declarations:
      font-size: 8pt
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Html,
    Svg,
    Json,
}

#[derive(Debug)]
struct Options {
    input: String,
    template: Option<String>,
    engine: Option<String>,
    format: Format,
    out: Option<String>,
    stats: bool,
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
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
}

fn parse_options(args: &[String], allow_format: bool) -> Result<Options, String> {
    let mut input: Option<String> = None;
    let mut template = None;
    let mut engine = None;
    let mut format = Format::Html;
    let mut out = None;
    let mut stats = false;

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
            "--template" | "-t" => template = Some(value()?),
            "--engine" => engine = Some(value()?),
            "--out" | "-o" => out = Some(value()?),
            "--stats" => stats = true,
            "--format" | "-f" if allow_format => {
                format = match value()?.as_str() {
                    "html" => Format::Html,
                    "svg" => Format::Svg,
                    "json" => Format::Json,
                    other => return Err(format!("unknown format `{other}`")),
                }
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
        index += 1;
    }

    Ok(Options {
        input: input.ok_or_else(|| format!("--input is required\n\n{USAGE}"))?,
        template,
        engine,
        format,
        out,
        stats,
    })
}

fn render(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, true)?;
    let (document, styled, engine, vertical) = pipeline(&options)?;
    let pages = layout(&document, &styled, &engine, vertical)?;
    let output = match options.format {
        Format::Html => render_html(&pages, styled.table(), &pages.text),
        Format::Svg => render_svg(&pages, styled.table(), &pages.text),
        Format::Json => {
            let mut json = pages.canonical_json();
            json.push('\n');
            json
        }
    };
    write_output(&options, &output)?;
    if options.stats {
        eprintln!(
            "pages={} blocks={} lines={} canonicalHash={:016x}",
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
            pages.hash()
        );
    }
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let options = parse_options(args, false)?;
    let (document, styled, engine, vertical) = pipeline(&options)?;
    let pages = layout(&document, &styled, &engine, vertical)?;
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

fn pipeline(
    options: &Options,
) -> Result<(Document, StyledDoc, BreakConfig, VerticalParams), String> {
    let source = std::fs::read_to_string(&options.input)
        .map_err(|error| format!("{}: {error}", options.input))?;
    let document = parse_document(&options.input, &source)?;

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
        None => (BreakConfig::default(), VerticalParams::default_v1()),
    };
    Ok((document, styled, engine, vertical))
}

fn parse_document(path: &str, source: &str) -> Result<Document, String> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "md" | "markdown" => Ok(soma_md::parse_markdown(source)),
        "html" | "htm" => Ok(soma_html::parse_html(source)),
        "tex" | "latex" => Ok(soma_latex::parse_latex(source)),
        other => Err(format!(
            "{path}: unsupported input `{other}` (expected md, html, or tex)"
        )),
    }
}

fn layout(
    document: &Document,
    styled: &StyledDoc,
    engine: &BreakConfig,
    vertical: VerticalParams,
) -> Result<soma_layout::PagedLayout, String> {
    let table = MetricsTable::embedded();
    let metrics = StyledMetrics::new(&table, styled);
    layout_with(
        document,
        styled,
        &metrics,
        engine,
        PaginationStrategy::VerticalKp,
        vertical,
    )
    .map_err(|error| error.to_string())
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
