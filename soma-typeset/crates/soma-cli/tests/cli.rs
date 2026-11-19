//! The CLI is a thin I/O shell: same pipeline as the gates, one command.

use std::path::PathBuf;
use std::process::Command;

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
    assert!(stdout.starts_with("{\"schema\":\"soma.pages/8\""));
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
    let input = temp("md-unknown", "docx", "whatever");
    let (ok, _stdout, stderr) = run(&["render", "--input", input.to_str().unwrap()]);

    assert!(!ok);
    assert!(stderr.contains("unsupported input"), "{stderr}");
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
    // Six chapters plus three level-2 sections at depth 2, one figure and one
    // table.
    assert!(
        stdout.contains("\"entries\":9"),
        "six chapters + three sections"
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
