//! End-to-end tests for source-level macro and counter expansion.

use soma_doc::inline::Inline;
use soma_doc::Block;
use soma_latex::parse_latex;

fn inline_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect(inlines, &mut out);
    out
}

fn collect(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&text.value),
            Inline::Code(code) => out.push_str(&code.value),
            Inline::Strong(children)
            | Inline::Emphasis(children)
            | Inline::Underline(children)
            | Inline::Strikethrough(children)
            | Inline::SmallCaps(children)
            | Inline::Superscript(children)
            | Inline::Subscript(children) => collect(&children.children, out),
            Inline::Link(link) => collect(&link.children, out),
            Inline::Math(math) => out.push_str(&math.source),
            Inline::Ruby(node) => out.push_str(&node.base),
            Inline::LineBreak(_) => out.push(' '),
            _ => {}
        }
    }
}

fn paragraph_text(block: &Block) -> String {
    let Block::Paragraph(paragraph) = block else {
        panic!("not a paragraph: {block:?}");
    };
    inline_text(&paragraph.inlines)
}

#[test]
fn zero_one_and_two_parameter_macros_expand() {
    let source = r#"\newcommand{\signature}{Soma}
\newcommand{\hello}[1]{你好，#1}
\newcommand{\pair}[2]{#1 与 #2}
\hello{世界}，\pair{甲{乙}丙}{丁}。\signature"#;
    let document = parse_latex(source);
    assert_eq!(
        paragraph_text(&document.blocks[0]),
        "你好，世界，甲乙丙 与 丁。Soma"
    );
    document.validate().expect("valid DocIR");
}

#[test]
fn nested_macros_and_renewcommand_take_effect() {
    let source = r"\newcommand{\inner}[1]{内#1}\newcommand{\outer}[1]{外\inner{#1}}\outer{x}\renewcommand{\inner}[1]{新#1}\outer{y}";
    let document = parse_latex(source);
    assert_eq!(paragraph_text(&document.blocks[0]), "外内x外新y");
    document.validate().expect("valid DocIR");
}

#[test]
fn providecommand_does_not_override_an_existing_macro() {
    let source =
        r"\providecommand{\x}{第一次}\providecommand{\x}{第二次}\x\providecommand{\y}{补}\y";
    let document = parse_latex(source);
    assert_eq!(paragraph_text(&document.blocks[0]), "第一次补");
    document.validate().expect("valid DocIR");
}

#[test]
fn def_declares_a_zero_parameter_macro() {
    let document = parse_latex(r"\def\hello{你好}\hello");
    assert_eq!(paragraph_text(&document.blocks[0]), "你好");
    document.validate().expect("valid DocIR");
}

#[test]
fn counters_step_and_render_in_every_style() {
    let source = concat!(
        r"\newcounter{score}\setcounter{score}{4}",
        r"\arabic{score} \roman{score} \Roman{score} ",
        r"\alph{score} \Alph{score} \value{score} \thescore ",
        r"\stepcounter{score}\arabic{score} ",
        r"\refstepcounter{score}\arabic{score}"
    );
    let document = parse_latex(source);
    assert_eq!(paragraph_text(&document.blocks[0]), "4 iv IV d D 4 4 5 6");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_new_counter_starts_at_zero() {
    let document = parse_latex(r"\newcounter{blank}\arabic{blank}");
    assert_eq!(paragraph_text(&document.blocks[0]), "0");
    document.validate().expect("valid DocIR");
}

#[test]
fn letter_counters_continue_past_z() {
    let source = concat!(
        r"\newcounter{n}\setcounter{n}{26}\alph{n}\Alph{n}",
        r"\setcounter{n}{27}\alph{n}\Alph{n}"
    );
    let document = parse_latex(source);
    assert_eq!(paragraph_text(&document.blocks[0]), "zZaaAA");
    document.validate().expect("valid DocIR");
}

#[test]
fn roman_counters_use_subtractive_notation() {
    let source = r"\newcounter{n}\setcounter{n}{1994}\roman{n} \Roman{n}";
    let document = parse_latex(source);
    assert_eq!(paragraph_text(&document.blocks[0]), "mcmxciv MCMXCIV");
    document.validate().expect("valid DocIR");
}

#[test]
fn definitions_disappear_and_commented_definitions_do_not_register() {
    let source = r#"\newcommand{\pre}{preamble}
% \newcommand{\hidden}{注释宏}
\begin{document}
\newcommand{\body}{body}
正文\pre \hidden{保留} \body
\end{document}"#;
    let document = parse_latex(source);
    assert_eq!(
        paragraph_text(&document.blocks[0]),
        "正文preamble 保留 body"
    );
    document.validate().expect("valid DocIR");
}

#[test]
fn an_escaped_percent_is_not_a_comment() {
    let document = parse_latex(r"50\% \newcommand{\x}{X}正文\x");
    assert_eq!(paragraph_text(&document.blocks[0]), "50% 正文X");
    document.validate().expect("valid DocIR");
}

#[test]
fn verbatim_content_is_not_expanded() {
    let source = r#"\newcommand{\x}{展开}
\begin{verbatim}
\x
\newcommand{\y}{Y}
\end{verbatim}
正文\y{保留}"#;
    let document = parse_latex(source);
    let Block::CodeBlock(code) = &document.blocks[0] else {
        panic!("verbatim code block");
    };
    assert_eq!(code.value, "\\x\n\\newcommand{\\y}{Y}");
    assert_eq!(paragraph_text(&document.blocks[1]), "正文保留");
    document.validate().expect("valid DocIR");
}

#[test]
fn a_recursive_macro_stops_without_panicking() {
    let document = parse_latex(r"\newcommand{\loop}{\loop}正文\loop结束");
    assert_eq!(paragraph_text(&document.blocks[0]), "正文结束");
    document.validate().expect("valid DocIR");
}

#[test]
fn macros_expand_inside_the_metadata() {
    let source = r"\newcommand{\book}{书名}\title{\book}\begin{document}正文\end{document}";
    let document = parse_latex(source);
    let title = document.meta.title.as_ref().expect("title");
    assert_eq!(inline_text(title), "书名");
    document.validate().expect("valid DocIR");
}

#[test]
fn an_undefined_command_keeps_its_braced_argument() {
    let document = parse_latex(r"\newcommand{\x}{X}\x \unknown{保留}");
    assert_eq!(paragraph_text(&document.blocks[0]), "X 保留");
    document.validate().expect("valid DocIR");
}
