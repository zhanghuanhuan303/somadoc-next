//! Lexer behaviour that layout and the three renderers rely on (ADR-0091).

use soma_core::SyntaxClass;
use soma_highlight::Lexer;

fn pairs(lang: &str, line: &str) -> Vec<(String, SyntaxClass)> {
    let chars: Vec<char> = line.chars().collect();
    let mut lexer = Lexer::new(Some(lang));
    lexer
        .line(line)
        .into_iter()
        .map(|span| (chars[span.start..span.end].iter().collect(), span.class))
        .collect()
}

fn has(pairs: &[(String, SyntaxClass)], text: &str, class: SyntaxClass) -> bool {
    pairs
        .iter()
        .any(|(seen, seen_class)| seen == text && *seen_class == class)
}

#[test]
fn rust_keywords_functions_types_and_comments() {
    let line = "fn main() { let x: u32 = 42; // note";
    let spans = pairs("rust", line);
    assert!(has(&spans, "fn", SyntaxClass::Keyword));
    assert!(has(&spans, "main", SyntaxClass::Function));
    assert!(has(&spans, "let", SyntaxClass::Keyword));
    assert!(has(&spans, "u32", SyntaxClass::Type));
    assert!(has(&spans, "42", SyntaxClass::Number));
    assert!(has(&spans, "// note", SyntaxClass::Comment));
}

#[test]
fn rust_strings_raw_strings_and_attributes() {
    let plain = pairs("rust", "let s = \"hi\";");
    assert!(has(&plain, "\"hi\"", SyntaxClass::String));

    let raw = pairs("rust", "let s = r#\"raw\"#;");
    assert!(
        raw.iter()
            .any(|(text, class)| text.contains("raw") && *class == SyntaxClass::String),
        "{raw:?}"
    );

    let attribute = pairs("rust", "#[derive(Debug)]");
    assert!(has(&attribute, "#[derive(Debug)]", SyntaxClass::Meta));
}

#[test]
fn rust_nested_block_comments_continue_across_lines() {
    let mut lexer = Lexer::new(Some("rust"));
    let first = lexer.line("/* outer /* inner");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].class, SyntaxClass::Comment);
    let second = lexer.line("still */ code */ let x = 1;");
    assert_eq!(second[0].class, SyntaxClass::Comment);
    assert!(second.iter().any(|span| span.class == SyntaxClass::Keyword));
}

#[test]
fn rust_raw_string_continues_across_lines() {
    let mut lexer = Lexer::new(Some("rust"));
    let first = lexer.line("let s = r#\"first");
    assert!(first.iter().any(|span| span.class == SyntaxClass::String));
    let line = lexer.line("second\"#; let x = 1;");
    assert_eq!(line[0].class, SyntaxClass::String);
    assert!(line.iter().any(|span| span.class == SyntaxClass::Keyword));
}

#[test]
fn c_and_cpp_classify_preprocessor_types_and_functions() {
    let c = pairs("c", "#include <stdio.h>");
    assert!(has(&c, "#include <stdio.h>", SyntaxClass::Meta));

    let c = pairs("c", "int main(void) { return 0; }");
    assert!(has(&c, "int", SyntaxClass::Type));
    assert!(has(&c, "main", SyntaxClass::Function));
    assert!(has(&c, "void", SyntaxClass::Type));
    assert!(has(&c, "return", SyntaxClass::Keyword));
    assert!(has(&c, "0", SyntaxClass::Number));

    let cpp = pairs("cpp", "std::vector<int> v; // ok");
    assert!(has(&cpp, "vector", SyntaxClass::Type));
    assert!(has(&cpp, "int", SyntaxClass::Type));
    assert!(has(&cpp, "// ok", SyntaxClass::Comment));
}

#[test]
fn shell_keywords_variables_and_comments() {
    let spans = pairs("shell", "for f in *.rs; do echo $HOME; done # note");
    assert!(has(&spans, "for", SyntaxClass::Keyword));
    assert!(has(&spans, "do", SyntaxClass::Keyword));
    assert!(has(&spans, "done", SyntaxClass::Keyword));
    assert!(has(&spans, "$HOME", SyntaxClass::Variable));
    assert!(has(&spans, "# note", SyntaxClass::Comment));
}

#[test]
fn shell_string_continues_across_lines() {
    let mut lexer = Lexer::new(Some("bash"));
    let first = lexer.line("echo \"hello");
    assert_eq!(first[0].class, SyntaxClass::String);
    let second = lexer.line("world\" # done");
    assert_eq!(second[0].class, SyntaxClass::String);
    assert!(second.iter().any(|span| span.class == SyntaxClass::Comment));
}

#[test]
fn markdown_headings_code_spans_and_emphasis() {
    let heading = pairs("md", "# Title");
    assert!(has(&heading, "#", SyntaxClass::Meta));
    assert!(has(&heading, "Title", SyntaxClass::Function));

    let inline = pairs("md", "a `code` and **bold**");
    assert!(has(&inline, "`code`", SyntaxClass::String));
    assert!(inline
        .iter()
        .any(|(text, class)| text == "**" && *class == SyntaxClass::Meta));

    let fence = pairs("md", "```rust");
    assert!(has(&fence, "```rust", SyntaxClass::Meta));
}

#[test]
fn latex_commands_environments_comments_and_math() {
    let line = r"\begin{document} \section{Hi} $x^2$ % note";
    let spans = pairs("latex", line);
    assert!(has(&spans, "\\begin", SyntaxClass::Keyword));
    assert!(has(&spans, "document", SyntaxClass::Type));
    assert!(has(&spans, "\\section", SyntaxClass::Keyword));
    assert!(has(&spans, "$x^2$", SyntaxClass::Literal));
    assert!(has(&spans, "% note", SyntaxClass::Comment));
}

#[test]
fn languages_and_aliases_select_a_profile() {
    assert_eq!(Lexer::new(Some("rs")).language_name(), "rust");
    assert_eq!(Lexer::new(Some("c++")).language_name(), "cpp");
    assert_eq!(Lexer::new(Some("bash")).language_name(), "shell");
    assert_eq!(Lexer::new(Some("markdown")).language_name(), "markdown");
    assert_eq!(Lexer::new(Some("tex")).language_name(), "latex");
    assert_eq!(Lexer::new(Some("rust,ignore")).language_name(), "rust");
    assert_eq!(
        Lexer::new(Some("rust {.line-numbers}")).language_name(),
        "rust"
    );
    assert_eq!(Lexer::new(Some("brainfuck")).language_name(), "plain");
    assert_eq!(Lexer::new(None).language_name(), "plain");
}

#[test]
fn unknown_language_is_plain_and_never_panics() {
    let mut lexer = Lexer::new(Some("brainfuck"));
    assert!(lexer.line("+++[>+<-]").is_empty());

    for input in [
        "",
        "\"unterminated",
        "/* unterminated",
        "中文 // 注释",
        "'lifetime",
        "r#\"unterminated",
        "```",
        "\\",
        "0x",
        "1e+",
    ] {
        let mut lexer = Lexer::new(Some("rust"));
        let _ = lexer.line(input);
    }
}

#[test]
fn every_character_is_covered_at_most_once() {
    let line = "let s = \"中文\"; // 注释";
    let mut lexer = Lexer::new(Some("rust"));
    let spans = lexer.line(line);
    let mut previous_end = 0;
    for span in &spans {
        assert!(span.start >= previous_end, "spans are sorted: {spans:?}");
        assert!(span.end > span.start);
        assert!(span.end <= line.chars().count());
        previous_end = span.end;
    }
}
