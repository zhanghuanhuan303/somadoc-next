//! The CLI is a thin I/O shell: same pipeline as the gates, one command.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_soma");

fn temp(name: &str, extension: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "soma-cli-{}-{}.{}",
        std::process::id(),
        name,
        extension
    ));
    std::fs::write(&path, content).expect("write fixture");
    path
}

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(BIN).args(args).output().expect("run soma");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> (bool, String, String) {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn soma");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("run soma");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("soma-cli-{}-dir-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create fixture directory");
    path
}

#[test]
fn markdown_renders_to_html_on_stdout() {
    let input = temp("md-html", "md", "# 标题\n\n这是中文正文，含 **强调**。\n");
    let (ok, stdout, stderr) = run(&["render", "--input", input.to_str().unwrap()]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("<!DOCTYPE html>"));
    assert!(stdout.contains("data-page=\"1\""));
    assert!(!stdout.contains("class=\"math\""));
}

#[test]
fn latex_math_reaches_the_renderer_end_to_end() {
    let input = temp(
        "tex-math",
        "tex",
        "行内 $a^2+b^2=c^2$ 与行间：\n\n\\[ \\frac{a}{b} \\]\n",
    );
    let (ok, stdout, stderr) = run(&["render", "--input", input.to_str().unwrap()]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("class=\"math\""), "inline formula spans");
    assert!(stdout.contains("class=\"math-rule\""), "the fraction bar");
}

#[test]
fn mixed_markdown_latex_html_reaches_the_renderer() {
    let input = temp(
        "mixed",
        "md",
        "行内 $x^2$ 与 \\textbf{粗体}，以及 <u>下划线</u>。\n\n$$y^2$$\n\n<div>HTML 段落里的 $z$</div>\n",
    );
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "html",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(
        stdout.matches("class=\"math\"").count() >= 3,
        "inline, display and HTML-block maths should all be painted: {stdout}"
    );
    assert!(!stdout.contains("textbf"), "LaTeX must not stay as source");
    assert!(
        !stdout.contains("&lt;u&gt;"),
        "HTML must not stay as source"
    );
    assert!(!stdout.contains("<script"), "no active content");
}

#[test]
fn json_is_the_canonical_page_result() {
    let input = temp("md-json", "md", "一段正文。\n");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.starts_with("{\"schema\":\"soma.pages/11\""));
    assert!(stdout.ends_with("}\n"));
}

#[test]
fn svg_is_available_and_draws_boxes() {
    let input = temp("md-svg", "md", "一段正文。\n");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "svg",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.starts_with("<svg"));
    assert!(stdout.contains("data-page=\"1\""));
}

#[test]
fn the_default_template_is_plex() {
    let input = temp("md-default", "md", "# 标题\n\n一段正文。\n");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    // The Plex reference: A4 with 25.4 mm margins. The content box is
    // 159.2 mm = 451.3 pt; the template asks for a TOC, which moves the body
    // to page 2.
    assert!(
        stdout.contains(
            "\"page\":{\"width\":243824,\"height\":344838,\"marginTop\":29491,\"marginBottom\":29491,\"marginLeft\":29491,\"marginRight\":29491}"
        ),
        "head: {stdout}"
    );
    // `numbering: headings: none` keeps the heading text bare.
    assert!(
        stdout.contains("\"outline\":[{\"level\":1,\"text\":\"标题\",\"page\":2"),
        "outline: {stdout}"
    );
    assert!(stdout.contains("\"toc\":{\"passes\":3"), "toc: {stdout}");
    assert!(
        stdout.contains("\"footer\":[{\"node\":"),
        "footer: {stdout}"
    );
    assert!(stdout.contains("\"printed\":\"2\""));
}

#[test]
fn the_archived_article_template_is_still_selectable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let template = root.join("examples/templates/latex-article.yaml");
    let input = temp("md-article", "md", "# 标题\n\n一段正文。\n");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--template",
        template.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(
        stdout.contains(
            "\"page\":{\"width\":243824,\"height\":344838,\"marginTop\":51551,\"marginBottom\":49229,\"marginLeft\":51087,\"marginRight\":51899}"
        ),
        "head: {stdout}"
    );
    assert!(stdout.contains("\"pageCount\":1"), "{stdout}");
    assert!(
        stdout.contains("\"outline\":[{\"level\":1,\"text\":\"1 标题\",\"page\":1"),
        "outline: {stdout}"
    );
}

#[test]
fn a_document_without_headings_gets_no_empty_contents_page() {
    let input = temp("md-no-toc", "md", "第一段。\n\n第二段。\n");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("\"pageCount\":1"), "{stdout}");
    assert!(
        stdout.contains("\"toc\":{\"passes\":1,\"stable\":true,\"entries\":0"),
        "{stdout}"
    );
}

#[test]
fn a_custom_template_changes_the_page_geometry() {
    let input = temp("md-template", "md", "一段正文。\n");
    let template = temp(
        "template",
        "yaml",
        r#"
name: tiny
page:
  size: custom
  width: 80mm
  height: 60mm
  margin: 5mm
styles:
  - selector:
      element: document
    declarations:
      font-size: 10pt
      line-height: 1.5
      align: justify
"#,
    );
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--template",
        template.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    // A4 is 232214 units wide; 80mm is 92885 (1em = 4096 units).
    assert!(stdout.contains("\"width\":92885"), "page width: {stdout}");
}

#[test]
fn out_writes_a_file_and_stdout_stays_empty() {
    let input = temp("md-out", "md", "一段正文。\n");
    let out = std::env::temp_dir().join(format!("soma-cli-{}-out.html", std::process::id()));
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "html",
        "--out",
        out.to_str().unwrap(),
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.is_empty());
    let written = std::fs::read_to_string(&out).expect("output file");
    assert!(written.contains("<!DOCTYPE html>"));
    let _ = std::fs::remove_file(out);
}

#[test]
fn check_reports_layout_facts() {
    let input = temp("md-check", "md", "第一段。\n\n第二段。\n");
    let (ok, stdout, stderr) = run(&["check", "--input", input.to_str().unwrap()]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("pages: 1"), "{stdout}");
    assert!(stdout.contains("canonical hash: "), "{stdout}");
}

#[test]
fn an_unknown_extension_is_a_clear_error() {
    let input = temp("md-unknown", "rtf", "whatever");
    let (ok, _stdout, stderr) = run(&["render", "--input", input.to_str().unwrap()]);

    assert!(!ok);
    assert!(stderr.contains("unsupported input"), "{stderr}");
}

#[test]
fn a_damaged_docx_is_a_clear_error() {
    let input = temp("md-bad-docx", "docx", "not a zip");
    let (ok, _stdout, stderr) = run(&["render", "--input", input.to_str().unwrap()]);

    assert!(!ok);
    assert!(stderr.contains("not a zip"), "{stderr}");
}

#[test]
fn a_missing_input_is_a_clear_error() {
    let (ok, _stdout, stderr) = run(&["render", "--input", "/nonexistent/nope.md"]);

    assert!(!ok);
    assert!(stderr.contains("/nonexistent/nope.md"), "{stderr}");
}

#[test]
fn a_missing_argument_is_a_usage_error() {
    let (ok, _stdout, stderr) = run(&["render"]);

    assert!(!ok);
    assert!(stderr.contains("--input is required"), "{stderr}");
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn help_lists_the_commands() {
    let (ok, stdout, _stderr) = run(&["--help"]);

    assert!(ok);
    assert!(stdout.contains("soma render"));
    assert!(stdout.contains("soma check"));
    assert!(stdout.contains("--from"));
    assert!(stdout.contains("--stats"));
    assert!(stdout.contains("latex-v1"));
    assert!(stdout.contains("stdin"));
    assert!(stdout.contains("directory"));
    assert!(
        stdout.contains("no --watch"),
        "watch mode is documented as absent"
    );
}

#[test]
fn version_prints_the_package_version() {
    let (ok, stdout, _stderr) = run(&["--version"]);
    assert!(ok);
    assert!(stdout.starts_with("soma "), "{stdout}");
    assert!(stdout.trim().len() > 5);
}

#[test]
fn stdin_markdown_renders_when_from_selects_the_frontend() {
    let (ok, stdout, stderr) = run_with_stdin(
        &["render", "--input", "-", "--from", "md"],
        "# 标题\n\n这是正文。\n".as_bytes(),
    );

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("<!DOCTYPE html>"), "stdout: {stdout}");
    // The renderer places each CJK glyph in its own span.
    assert!(
        stdout.contains('标') && stdout.contains('题'),
        "stdout: {stdout}"
    );
}

#[test]
fn stdin_without_from_is_a_clear_error() {
    let (ok, _stdout, stderr) = run_with_stdin(&["render", "--input", "-"], b"");

    assert!(!ok, "a stream cannot pick a frontend by extension");
    assert!(stderr.contains("--from"), "stderr: {stderr}");
}

#[test]
fn a_directory_renders_each_supported_file_into_the_out_directory() {
    let dir = temp_dir("batch");
    std::fs::write(dir.join("a.md"), "# 甲\n\n正文。\n").expect("write a.md");
    std::fs::write(dir.join("b.html"), "<p>乙</p>").expect("write b.html");
    std::fs::write(dir.join("ignored.txt"), "not a document").expect("write ignored.txt");
    let nested = dir.join("nested");
    std::fs::create_dir_all(&nested).expect("create nested directory");
    std::fs::write(nested.join("c.md"), "# 丙\n").expect("write nested c.md");

    let out = dir.join("out");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        dir.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.is_empty(), "batch mode writes files, not stdout");
    let a = std::fs::read_to_string(out.join("a.html")).expect("a.html");
    assert!(a.contains("甲"), "a.html: {a}");
    let b = std::fs::read_to_string(out.join("b.html")).expect("b.html");
    assert!(b.contains("乙"), "b.html: {b}");
    assert!(
        !out.join("ignored.html").exists(),
        "unsupported files are skipped"
    );
    assert!(!out.join("c.html").exists(), "batch mode is non-recursive");

    let json_out = dir.join("out-json");
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        dir.to_str().unwrap(),
        "--format",
        "json",
        "--out",
        json_out.to_str().unwrap(),
    ]);
    assert!(ok, "stderr: {stderr}");
    assert!(stdout.is_empty());
    let json = std::fs::read_to_string(json_out.join("a.json")).expect("a.json");
    assert!(json.starts_with("{\"schema\":\"soma.pages/"), "{json}");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_directory_without_out_is_an_error() {
    let dir = temp_dir("batch-no-out");
    std::fs::write(dir.join("a.md"), "正文。\n").expect("write a.md");

    let (ok, _stdout, stderr) = run(&["render", "--input", dir.to_str().unwrap()]);

    assert!(!ok);
    assert!(stderr.contains("--out"), "stderr: {stderr}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_directory_batch_refuses_to_overwrite_an_input() {
    let dir = temp_dir("batch-overwrite");
    std::fs::write(dir.join("a.html"), "<p>原文</p>").expect("write a.html");

    let (ok, _stdout, stderr) = run(&[
        "render",
        "--input",
        dir.to_str().unwrap(),
        "--out",
        dir.to_str().unwrap(),
    ]);

    assert!(!ok);
    assert!(stderr.contains("overwrite"), "stderr: {stderr}");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.html")).expect("a.html"),
        "<p>原文</p>",
        "the source is untouched"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_toc_template_lists_the_headings() {
    let input = temp("md-toc", "md", "# 第一章\n\n正文。\n\n# 第二章\n\n正文。\n");
    let template = temp(
        "toc-template",
        "yaml",
        r#"
name: with-toc
page:
  size: A5
  margin: 15mm
toc:
  depth: 2
  title: 目录
styles:
  - selector:
      element: document
    declarations:
      font-size: 10.5pt
      line-height: 1.6
      align: justify
  - selector:
      element: toc
    declarations:
      font-size: 9pt
"#,
    );
    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--template",
        template.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("\"toc\":{\"passes\":"), "{stdout}");
    assert!(stdout.contains("\"stable\":true"), "{stdout}");
    assert!(stdout.contains("\"entries\":2"), "{stdout}");
    assert!(stdout.contains("\"pageCount\":2"), "{stdout}");
}

#[test]
fn the_example_book_renders_cover_contents_and_chapters() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let input = root.join("examples/book/book.tex");
    let template = root.join("examples/book/book.yaml");

    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--template",
        template.to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert!(ok, "stderr: {stderr}");
    // Cover page, contents page, and one page per chapter (6 chapters, two of
    // them spilling onto a second page): 10 pages today. The assertion is a
    // range so content edits do not need a test edit.
    assert!(stdout.contains("\"toc\":{\"passes\":"), "{stdout}");
    assert!(stdout.contains("\"stable\":true"), "the TOC converged");
    // Six chapters, three level-2 sections, and the generated index, plus one
    // figure and one table.
    assert!(
        stdout.contains("\"entries\":10"),
        "chapters + sections + index"
    );
    assert!(stdout.contains("\"figures\":1"), "one figure in the list");
    assert!(stdout.contains("\"tables\":1"), "one table in the list");
    assert!(!stdout.contains("\"overflow\":true"), "no page overflows");
    assert!(
        !stdout.contains("\"stage\":\"emergency\"") && !stdout.contains("\"stage\":\"overfull\""),
        "the book lays out without a degraded line"
    );
    let pages: u32 = stdout
        .split("\"pageCount\":")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|value| value.parse().ok())
        .expect("pageCount");
    assert!((8..=12).contains(&pages), "page count {pages}");
}

#[test]
fn pdf_output_embeds_a_font_and_bookmarks() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let font = root.join("crates/soma-render-pdf/tests/fixtures/tiny.ttf");
    let input = temp("pdf", "md", "# A中B\n\nA B 中\n");
    let out = std::env::temp_dir().join(format!("soma-cli-{}-pdf-out.pdf", std::process::id()));

    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "pdf",
        "--font",
        font.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.is_empty(), "the binary goes to the file");
    let bytes = std::fs::read(&out).expect("pdf output");
    assert!(bytes.starts_with(b"%PDF-1.7"), "header");
    assert!(
        bytes.windows(10).any(|window| window == b"/FontFile2"),
        "embedded font"
    );
    assert!(
        bytes.windows(9).any(|window| window == b"/Outlines"),
        "bookmarks"
    );
    let _ = std::fs::remove_file(out);
}

#[test]
fn pdf_without_a_font_is_an_error() {
    let input = temp("pdf-no-font", "md", "一段正文。\n");
    let (ok, _stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "pdf",
    ]);

    assert!(!ok, "pdf needs a font");
    assert!(stderr.contains("--font"), "stderr: {stderr}");
}

/// The face the table was measured from is bundled, so the default path works
/// on a machine with no TeX installation and never lets a *different* math
/// face stand in (the table's variants are addressed by ordinal, ADR-0077).
#[test]
fn the_bundled_math_face_is_used_without_the_flag() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let font = root.join("crates/soma-render-pdf/tests/fixtures/tiny.ttf");
    let input = temp(
        "pdf-auto-math",
        "tex",
        "行内 $\\sum_{i=1}^{n} i$ 与 $\\sqrt{2}$。\n\n\\[ \\frac{a}{b} \\]\n",
    );
    let out =
        std::env::temp_dir().join(format!("soma-cli-{}-pdf-auto-math.pdf", std::process::id()));

    let (ok, stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "pdf",
        "--font",
        font.to_str().unwrap(),
        "--stats",
        "--out",
        out.to_str().unwrap(),
    ]);

    assert!(ok, "stderr: {stderr}");
    assert!(stdout.is_empty(), "pdf goes to the file");
    assert!(
        stderr.contains("math:") && stderr.contains("(auto)"),
        "the auto-picked face is reported under --stats: {stderr}"
    );
    let bytes = std::fs::read(&out).expect("pdf output");
    // The tiny body face is TrueType (`/FontFile2`); Latin Modern Math is CFF
    // (`/FontFile3`). Both programs are embedded.
    let programs = bytes
        .windows(10)
        .filter(|window| window == b"/FontFile2")
        .count()
        + bytes
            .windows(10)
            .filter(|window| window == b"/FontFile3")
            .count();
    assert!(
        programs >= 2,
        "body and math programs are both embedded: {programs}"
    );
    let _ = std::fs::remove_file(out);
}

#[test]
fn an_explicit_metrics_table_matches_the_embedded_one() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let metrics = root.join("crates/soma-metrics/data/noto-cjk.smt");
    let input = temp(
        "metrics",
        "md",
        "中文排版测试，含 Latin words 和数字 123。\n",
    );

    let (ok, embedded, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(ok, "stderr: {stderr}");
    let (ok, explicit, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--format",
        "json",
        "--metrics",
        metrics.to_str().unwrap(),
    ]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(embedded, explicit, "the file and embedded table agree");
}

#[test]
fn a_missing_metrics_file_is_an_error() {
    let input = temp("metrics-missing", "md", "正文。\n");
    let (ok, _stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--metrics",
        "/nonexistent/metrics.smt",
    ]);

    assert!(!ok);
    assert!(stderr.contains("metrics.smt"), "stderr: {stderr}");
}
